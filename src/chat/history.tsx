import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { History as HistoryIcon, LoaderCircle } from 'lucide-react';
import { ago } from './format';

type Session = { id: string; title: string; modified: number };

// The workspace's sessions, read each time the list opens.
function SessionList({ current, container, onPick, onClose }: { current: string; container: React.RefObject<HTMLDivElement | null>; onPick: (id: string) => void; onClose: () => void }) {
  const [sessions, setSessions] = useState<Session[] | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    invoke<Session[]>('chat_sessions').then(setSessions).catch(reason => setError(String(reason)));
    const outside = (event: PointerEvent) => { if (!container.current!.contains(event.target as Node)) onClose(); };
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('keydown', escape);
    return () => { document.removeEventListener('pointerdown', outside); document.removeEventListener('keydown', escape); };
  }, []);
  return <div className="history-popover" role="dialog" aria-label="历史对话">
    {error ? <div className="history-status failed">{error}</div>
      : sessions === null ? <div className="history-status"><LoaderCircle className="spin" size={14}/>正在读取历史对话</div>
      : sessions.length === 0 ? <div className="history-status">还没有历史对话</div>
      : sessions.map(session => <button key={session.id} className={`history-item ${session.id === current ? 'current' : ''}`} title={session.title} onClick={() => onPick(session.id)}>
        <span>{session.title}</span>{session.id === current && <b>当前</b>}<small>{ago(session.modified)}</small>
      </button>)}
  </div>;
}

export function History({ current, onPick }: { current: string; onPick: (id: string) => void }) {
  const [open, setOpen] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  return <div className="history" ref={container}>
    <button className="tool-button" aria-expanded={open} onClick={() => setOpen(!open)}><HistoryIcon size={15}/>历史</button>
    {open && <SessionList current={current} container={container} onClose={() => setOpen(false)}
      onPick={id => { setOpen(false); if (id !== current) onPick(id); }}/>}
  </div>;
}
