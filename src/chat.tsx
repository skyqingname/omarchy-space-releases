import React, { useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow, UserAttentionType } from '@tauri-apps/api/window';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { Globe, MessageSquare, Monitor, Plus, RotateCcw, SquareTerminal } from 'lucide-react';
import product from '../product.config.json';
import { conversation, initial, opensDialog, phase as phaseOf, reduce } from './chat/model';
import type { ChatEvent, Phase } from './chat/model';
import { Transcript } from './chat/transcript';
import { Composer } from './chat/composer';
import { History } from './chat/history';

// The chat is a view over Claude Code running in the VM terminal. Rust
// reduces Claude's transcript, hooks and status line to events; the terminal
// stays the source, and what the chat sends goes into it.
const phaseText: Record<Phase, string> = { exited: '已退出', waiting: '等你回答', working: '正在工作', idle: '空闲' };

export function ChatPane({ visible, user, alert, onBrowser }: { visible: boolean; user: string; alert: React.ReactNode; onBrowser: () => void }) {
  const [state, dispatch] = useReducer(reduce, initial);
  const [mode, setMode] = useState<'chat'|'terminal'>('chat');
  const [error, setError] = useState('');
  const [now, setNow] = useState(Date.now);
  const terminalBox = useRef<HTMLDivElement>(null);
  const terminal = useRef<Terminal|null>(null);
  const fit = useRef<FitAddon|null>(null);
  const generation = useRef(0);
  const autoTerminal = useRef(false);
  const activity = useRef(state.activity);

  // `session` continues that one; otherwise the last one, or a new one when `fresh`.
  async function connect({ session, fresh = false }: { session?: string; fresh?: boolean }) {
    const current = Date.now();
    generation.current = current;
    dispatch({ type: 'clear' }); setError('');
    terminal.current!.reset();
    try { await invoke('chat_open', { generation: current, session: session ?? null, fresh, cols: terminal.current!.cols, rows: terminal.current!.rows }); }
    catch (reason) { if (generation.current === current) { setError(String(reason)); dispatch({ type: 'exit', now: Date.now() }); } }
  }

  useEffect(() => {
    const term = new Terminal({ fontFamily: "Menlo, 'SF Mono', Consolas, 'Courier New', monospace", fontSize: 13, cursorBlink: true, scrollback: 5000,
      theme: { background: '#1f2620', foreground: '#e8ecdf', cursor: '#d7e8ad', selectionBackground: '#4d6141' } });
    const fitter = new FitAddon();
    term.loadAddon(fitter);
    term.open(terminalBox.current!);
    fitter.fit();
    terminal.current = term; fit.current = fitter;
    term.onData(data => { invoke('terminal_input', { data }).catch(reason => setError(String(reason))); });
    const resize = new ResizeObserver(() => {
      if (!terminalBox.current!.clientWidth) return;
      fitter.fit();
      invoke('terminal_resize', { cols: term.cols, rows: term.rows }).catch(() => undefined);
    });
    resize.observe(terminalBox.current!);
    const listeners = Promise.all([
      listen<{ generation: number; data: string }>('terminal', ({ payload }) => {
        if (payload.generation === generation.current) term.write(Uint8Array.from(atob(payload.data), character => character.charCodeAt(0)));
      }),
      listen<ChatEvent>('chat', ({ payload }) => { if (payload.generation === generation.current) dispatch({ type: 'event', event: payload, now: Date.now() }); }),
      listen<{ generation: number }>('chat-exit', ({ payload }) => { if (payload.generation === generation.current) dispatch({ type: 'exit', now: Date.now() }); }),
    ]);
    listeners.then(() => connect({}));
    return () => { resize.disconnect(); listeners.then(list => list.forEach(stop => stop())); term.dispose(); invoke('chat_close'); };
  }, []);

  // Before Claude is signed in and trusts the folder, its own screens are the next step.
  const { status } = state;
  const signedIn = status === null || (status.signed_in && status.onboarded);
  const ready = signedIn && (status === null || status.trusted);
  useEffect(() => {
    if (!ready) { autoTerminal.current = true; setMode('terminal'); }
    else if (autoTerminal.current) { autoTerminal.current = false; setMode('chat'); }
  }, [ready]);
  useEffect(() => {
    if (visible && mode === 'terminal') { fit.current!.fit(); terminal.current!.focus(); }
  }, [visible, mode]);

  const talk = useMemo(() => conversation(state.items, state.live, state.activity), [state.items, state.live, state.activity]);
  const phase = phaseOf(state);
  // Running timers tick only while something runs.
  const running = phase === 'working' || phase === 'waiting';
  useEffect(() => {
    if (!running) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), product.chat.tick_milliseconds);
    return () => window.clearInterval(timer);
  }, [running]);
  // Claude finished or needs an answer while the user is elsewhere.
  useEffect(() => {
    if (activity.current === 'working' && !document.hasFocus())
      getCurrentWindow().requestUserAttention(UserAttentionType.Informational).catch(reason => setError(String(reason)));
    activity.current = state.activity;
  }, [state.activity]);

  async function send(text: string) {
    // These open a dialog in the TUI, which is answered there and may record nothing.
    const dialog = opensDialog(text, product.chat.slash_commands);
    const current = generation.current;
    await invoke('chat_send', { text });
    if (generation.current !== current) return;
    if (!dialog) dispatch({ type: 'send', text, now: Date.now() });
    if (dialog) setMode('terminal');
  }
  function interrupt() {
    dispatch({ type: 'stop', now: Date.now() });
    invoke('terminal_input', { data: '\u001b' }).catch(reason => setError(String(reason)));
  }
  // The TUI takes an option's number as its answer.
  const answer = (index: number) => invoke('terminal_input', { data: String(index + 1) }).catch(reason => setError(String(reason)));
  const openLink = (address: string) => invoke('chat_open_link', { address }).then(() => onBrowser()).catch(reason => setError(String(reason)));
  const showDesktop = () => invoke('show_desktop').catch(reason => setError(String(reason)));
  const toTerminal = () => setMode('terminal');

  return <section className={`chat-pane ${visible ? '' : 'hidden'}`} aria-label="Claude 对话">
    <div className="chat-toolbar">
      <span className={`chat-state ${phase}`}><span className="status-dot"/>Claude Code<span>{phaseText[phase]}</span></span>
      {state.title && <span className="chat-title" title={state.title}>{state.title}</span>}
      <div className="segmented" role="tablist">
        <button role="tab" aria-selected={mode === 'chat'} className={mode === 'chat' ? 'active' : ''} onClick={() => setMode('chat')}><MessageSquare size={14}/>对话</button>
        <button role="tab" aria-selected={mode === 'terminal'} className={mode === 'terminal' ? 'active' : ''} onClick={toTerminal}><SquareTerminal size={14}/>终端</button>
      </div>
      <div className="chat-actions">
        <History current={state.session} onPick={session => connect({ session })}/>
        <button className="tool-button" onClick={() => connect({ fresh: true })}><Plus size={15}/>新对话</button>
        <button className="tool-button" onClick={showDesktop}><Monitor size={15}/>打开桌面</button>
      </div>
    </div>
    {alert}
    {error && <div role="alert" className="error-banner chat-error"><span>{error}</span></div>}
    {!signedIn && <div className="chat-notice">首次使用先登录 Claude：在终端里按提示操作，登录页会在「浏览器」里打开。<button className="text-button" onClick={onBrowser}>打开浏览器 <Globe size={14}/></button></div>}
    {signedIn && !ready && <div className="chat-notice">第一次在这个工作目录使用，Claude 会先问是否信任它：在终端里选 “Yes, I trust this folder” 再回车。</div>}
    <div className="chat-body">
      <div className={`chat-view ${mode === 'chat' ? '' : 'concealed'}`}>
        <Transcript state={state} talk={talk} phase={phase} now={now} onTerminal={toTerminal} onAnswer={answer} onLink={openLink}>
          {state.exited && <div className="chat-ended"><span>Claude 已退出。</span><button className="secondary" onClick={() => connect({})}><RotateCcw size={15}/>继续这次对话</button><button className="secondary" onClick={() => connect({ fresh: true })}><Plus size={15}/>新对话</button></div>}
        </Transcript>
        <Composer session={state.session} enabled={!state.exited && ready && state.session !== ''} working={phase === 'working'} visible={visible && mode === 'chat'} user={user} meter={state.meter}
          placeholder={!signedIn ? '先在终端里完成 Claude 登录' : !ready ? '先在终端里信任这个工作目录' : '输入要 Claude 做的事，Enter 发送，Shift + Enter 换行；/ 使用命令'}
          onSend={send} onStop={interrupt}/>
      </div>
      <div className={`terminal-view ${mode === 'terminal' ? '' : 'concealed'}`}><div className="terminal-box" ref={terminalBox}/></div>
    </div>
  </section>;
}
