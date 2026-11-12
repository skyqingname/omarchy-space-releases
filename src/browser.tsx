import React, { useEffect, useRef, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import RFB from '@novnc/novnc';
import { LoaderCircle, RotateCcw } from 'lucide-react';
import product from '../product.config.json';

// The browser runs in the VM on a screen of its own; this page shows that
// screen. Its clipboard never reaches the host: noVNC's clipboard event is
// not handled, and host text reaches the VM through the one-way forwarder.
type Size = { width: number; height: number; scale: number };
type KeyHandler = (keysym: number, code: string, down: boolean, ...rest: unknown[]) => void;
type MessageHandler = (event: { data: ArrayBuffer }) => void;

// noVNC accepts anything shaped like a WebSocket. This one carries the VNC
// bytes over Tauri's binary channel to the forwarded wayvnc connection, and
// sends input in order, one call after the other.
class VncChannel {
  binaryType = 'arraybuffer';
  protocol = '';
  readyState = 'connecting';
  onopen: (() => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  onclose: ((event: { code: number; reason: string; wasClean: boolean }) => void) | null = null;
  readonly frames = new Channel<ArrayBuffer>();
  private receive: MessageHandler | null = null;
  private early: ArrayBuffer[] = [];
  private sending: Promise<void> = Promise.resolve();

  constructor() {
    this.frames.onmessage = data => { if (this.receive) this.receive({ data }); else this.early.push(data); };
  }
  get onmessage() { return this.receive; }
  // The server speaks first; its bytes wait until noVNC has attached and opened.
  set onmessage(handler: MessageHandler | null) {
    this.receive = handler;
    window.setTimeout(() => this.early.splice(0).forEach(data => this.receive!({ data })));
  }
  send(data: Uint8Array) {
    const copy = data.slice();
    this.sending = this.sending.then(() => invoke<void>('browser_send', copy)).catch(() => this.end());
  }
  close() { this.readyState = 'closed'; }
  end() {
    if (this.readyState === 'closed') return;
    this.readyState = 'closed';
    this.onclose?.({ code: 1006, reason: '', wasClean: false });
  }
}

// Command arrives as Super, which Omarchy keeps for window management. In the
// browser a Mac user means the browser's Ctrl shortcuts (copy, new tab, close tab).
const commandToControl: Record<number, [number, string]> = {
  0xffeb: [0xffe3, 'ControlLeft'], 0xffe7: [0xffe3, 'ControlLeft'],
  0xffec: [0xffe4, 'ControlRight'], 0xffe8: [0xffe4, 'ControlRight'],
};

export function BrowserPane({ visible, platform }: { visible: boolean; platform: string }) {
  const screen = useRef<HTMLDivElement>(null);
  const client = useRef<RFB|null>(null);
  const channel = useRef<VncChannel|null>(null);
  const generation = useRef(0);
  const [state, setState] = useState<'connecting'|'connected'|'closed'>('connecting');
  // Omarchy's bar keeps [left, top, right, bottom] of every screen. The screen
  // is requested that much larger and shifted so the bar falls outside the view.
  const [reserved, setReserved] = useState([0, 0, 0, 0]);
  const [error, setError] = useState('');

  // Rendered at whole device pixels so text stays sharp on high-density screens.
  function measure(): Size {
    const scale = Math.max(1, Math.round(window.devicePixelRatio));
    const width = screen.current!.clientWidth || product.browser.initial_width;
    const height = screen.current!.clientHeight || product.browser.initial_height;
    return { width: width * scale, height: height * scale, scale };
  }

  async function connect() {
    const current = Date.now();
    generation.current = current;
    client.current?.disconnect();
    client.current = null;
    setState('connecting'); setError('');
    try {
      const vnc = new VncChannel();
      channel.current = vnc;
      await invoke('browser_open', { generation: current, size: measure(), frames: vnc.frames });
      if (generation.current !== current) return;
      vnc.readyState = 'open';
      const rfb = new RFB(screen.current!, vnc, { shared: true });
      rfb.scaleViewport = true;
      rfb.resizeSession = false;
      rfb.focusOnClick = true;
      rfb.qualityLevel = product.browser.quality_level;
      rfb.compressionLevel = product.browser.compression_level;
      if (platform === 'macos-aarch64') {
        const keyboard = (rfb as unknown as { _keyboard: { onkeyevent: KeyHandler } })._keyboard;
        const forward = keyboard.onkeyevent;
        keyboard.onkeyevent = (keysym, code, down, ...rest) => {
          const [mapped, mappedCode] = commandToControl[keysym] ?? [keysym, code];
          forward(mapped, mappedCode, down, ...rest);
        };
      }
      rfb.addEventListener('connect', () => setState('connected'));
      rfb.addEventListener('disconnect', () => { if (generation.current === current) setState('closed'); });
      client.current = rfb;
    } catch (reason) {
      if (generation.current === current) { setError(String(reason)); setState('closed'); }
    }
  }

  useEffect(() => {
    const layout = listen<{ generation: number; reserved: number[] }>('browser-layout', ({ payload }) => {
      if (payload.generation === generation.current) setReserved(payload.reserved);
    });
    const exit = listen<{ generation: number }>('browser-exit', ({ payload }) => {
      if (payload.generation !== generation.current) return;
      channel.current?.end();
      setState('closed');
    });
    connect();
    let timer = 0;
    const resize = new ResizeObserver(() => {
      if (!screen.current!.clientWidth) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => { invoke('browser_resize', { size: measure() }).catch(() => undefined); }, product.browser.resize_debounce_milliseconds);
    });
    resize.observe(screen.current!);
    return () => {
      generation.current = 0;
      resize.disconnect(); window.clearTimeout(timer);
      client.current?.disconnect();
      exit.then(stop => stop());
      layout.then(stop => stop());
      invoke('browser_close');
    };
  }, []);
  useEffect(() => { if (visible && state === 'connected') client.current!.focus(); }, [visible, state]);

  return <section className={`browser-pane ${visible ? '' : 'hidden'}`} aria-label="虚拟机浏览器">
    <div className="chat-toolbar">
      <span className={`chat-state ${state === 'connected' ? '' : state === 'connecting' ? 'working' : 'ended'}`}><span className="status-dot"/>虚拟机浏览器<span>{state === 'connected' ? '通过所选节点上网' : state === 'connecting' ? '正在连接' : '已断开'}</span></span>
      <div className="chat-actions">{state === 'closed' && <button className="tool-button" onClick={connect}><RotateCcw size={15}/>重新连接</button>}</div>
    </div>
    {error && <div role="alert" className="error-banner chat-error"><span>{error}</span></div>}
    <div className="browser-body">
      <div className="browser-screen" ref={screen} style={{ left: -reserved[0], top: -reserved[1], right: -reserved[2], bottom: -reserved[3] }}/>
      {state === 'connecting' && <div className="browser-wait"><LoaderCircle className="spin" size={18}/>正在打开虚拟机里的浏览器</div>}
    </div>
  </section>;
}
