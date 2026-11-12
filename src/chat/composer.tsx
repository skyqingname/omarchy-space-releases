import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { ArrowUp, Paperclip, ShieldAlert, Square } from 'lucide-react';
import { AttachmentList, useAttachments } from './attachments';
import { attachmentMessage } from './attachment-model';
import product from '../../product.config.json';
import { slashMatches, usedTokens } from './model';
import type { Meter } from './model';
import { resetTime } from './format';

// Unsent text survives restarts, kept per session. The page wrote it, so it is read as it was written.
const drafts: Record<string, string> = JSON.parse(localStorage.getItem(product.chat.draft_storage_key) ?? '{}');
function store(session: string, text: string) {
  if (text) drafts[session] = text;
  else delete drafts[session];
  localStorage.setItem(product.chat.draft_storage_key, JSON.stringify(drafts));
}

// The session's draft, written a moment after the last edit.
function useDraft(session: string) {
  const [draft, setDraft] = useState('');
  const timer = useRef(0);
  const unsaved = useRef<[string, string] | null>(null);
  const flush = () => {
    window.clearTimeout(timer.current);
    if (unsaved.current) store(...unsaved.current);
    unsaved.current = null;
  };
  useEffect(() => { flush(); setDraft(drafts[session] ?? ''); }, [session]);
  useEffect(() => flush, []);
  function change(text: string) {
    setDraft(text);
    unsaved.current = [session, text];
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(flush, product.chat.draft_save_milliseconds);
  }
  function clear() {
    window.clearTimeout(timer.current);
    unsaved.current = null;
    setDraft('');
    store(session, '');
  }
  return { draft, change, clear };
}

const tokens = (count: number) => count.toLocaleString('zh-CN');

// What Claude's status line last reported: model, effort, context window and usage limits.
function MeterView({ meter }: { meter: Meter }) {
  const { model, effort, context_window: context, rate_limits: limits } = meter;
  const used = context?.used_percentage ?? null;
  const level = used === null ? '' : used >= product.chat.context_full_percentage ? 'full' : used >= product.chat.context_high_percentage ? 'high' : '';
  return <span className="meter">
    {model && <span>{model.display_name}</span>}
    {effort && <span>{effort.level}</span>}
    {context && used !== null && <span className={`context-meter ${level}`} title={context.current_usage ? `${tokens(usedTokens(context.current_usage))} / ${tokens(context.context_window_size)} tokens` : `${tokens(context.context_window_size)} tokens`}>
      <svg className="context-ring" viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="7.5" pathLength={100}/><circle cx="10" cy="10" r="7.5" pathLength={100} strokeDasharray={`${used} 100`}/></svg>
      {Math.round(used)}%
    </span>}
    {limits && <span title={`5 小时额度 ${resetTime(limits.five_hour.resets_at)} 重置\n本周额度 ${resetTime(limits.seven_day.resets_at)} 重置`}>
      5 小时 {Math.round(limits.five_hour.used_percentage)}% · 本周 {Math.round(limits.seven_day.used_percentage)}%
    </span>}
  </span>;
}

type Props = {
  session: string; enabled: boolean; working: boolean; visible: boolean; user: string; meter: Meter | null; placeholder: string;
  onSend: (text: string) => Promise<void>; onStop: () => void;
};

export function Composer({ session, enabled, working, visible, user, meter, placeholder, onSend, onStop }: Props) {
  const { draft, change, clear } = useDraft(session);
  const [pick, setPick] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState('');
  const attachments = useAttachments(session, enabled && !sending, visible);
  const currentSession = useRef(session);
  currentSession.current = session;
  const textarea = useRef<HTMLTextAreaElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const commands = dismissed || attachments.jobs.length || sending ? [] : slashMatches(draft, product.chat.slash_commands);
  const at = Math.min(pick, commands.length - 1);
  useLayoutEffect(() => {
    const box = textarea.current!;
    box.style.height = 'auto';
    box.style.height = `${box.scrollHeight}px`;
  }, [draft, visible]);
  useEffect(() => { menu.current?.querySelector('.active')?.scrollIntoView({ block: 'nearest' }); }, [at]);

  const canSend = enabled && !sending && !attachments.blocked && (draft.trim().length > 0 || attachments.jobs.length > 0);
  useEffect(() => { setSendError(''); }, [session]);
  async function send(text: string) {
    if (!canSend) return;
    if (attachments.jobs.length && /^[\/!]/.test(text.trim())) {
      setSendError('附件请随普通消息发送；执行命令前请先移除附件。'); return;
    }
    const target = session;
    const jobs = [...attachments.jobs];
    setSending(true); setSendError('');
    try {
      await onSend(attachmentMessage(text, jobs));
      attachments.queue.consume(jobs.map(job => job.id));
      if (currentSession.current === target) clear();
      else store(target, '');
    } catch (reason) { setSendError(String(reason)); }
    finally { setSending(false); }
  }
  function edit(text: string) {
    change(text);
    setPick(0);
    setDismissed(false);
  }
  function keyed(event: React.KeyboardEvent) {
    if (event.nativeEvent.isComposing) return;
    if (commands.length && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
      event.preventDefault();
      setPick((at + (event.key === 'ArrowDown' ? 1 : commands.length - 1)) % commands.length);
    } else if (commands.length && event.key === 'Tab' && !event.shiftKey) {
      event.preventDefault();
      edit(`/${commands[at].name} `);
    } else if (commands.length && event.key === 'Escape') {
      event.preventDefault();
      setDismissed(true);
    } else if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      if (commands.length && canSend) void send(`/${commands[at].name}`);
      else if (canSend) send(draft.trim());
    }
  }

  return <div className={`composer-wrap ${attachments.hovering ? 'file-hover' : ''}`}>
    {attachments.hovering && <div className="attachment-hint">松开鼠标，将文件传入虚拟机并添加到这条消息</div>}
    <AttachmentList jobs={attachments.jobs} onRemove={id => attachments.queue.remove(id)} onRetry={id => attachments.queue.retry(id)}/>
    {(attachments.error || sendError) && <div className="attachment-error" role="alert">{attachments.error || sendError}</div>}
    <div className="composer">
      {commands.length > 0 && enabled && <div className="slash-menu" role="listbox" aria-label="命令" ref={menu}>
        {commands.map((command, index) => <button key={command.name} role="option" aria-selected={index === at} className={`slash-item ${index === at ? 'active' : ''}`}
          // The message box keeps the focus, and with it the keys.
          onMouseDown={event => event.preventDefault()} onMouseEnter={() => setPick(index)} onClick={() => send(`/${command.name}`)}>
          <code>/{command.name}</code><span>{command.description}</span>
        </button>)}
      </div>}
      <button className="tool-button attach-button" aria-label="添加附件" title={`添加附件，支持多选或拖入；单文件最多 ${product.attachments.max_file_bytes / 1048576} MiB`} disabled={!enabled || sending || attachments.choosing} onClick={attachments.choose}><Paperclip size={18}/></button>
      <textarea ref={textarea} aria-label="给 Claude 的消息" rows={1} value={draft} disabled={!enabled || sending} placeholder={placeholder}
        onChange={event => edit(event.target.value)} onKeyDown={keyed}/>
      {working && !draft.trim() && attachments.jobs.length === 0
        ? <button className="send stop" aria-label="停止" onClick={onStop}><Square size={14}/></button>
        : <button className="send" aria-label="发送" disabled={!canSend} onClick={() => send(draft.trim())}><ArrowUp size={17}/></button>}
    </div>
    <div className="composer-meta">
      <span className="access"><ShieldAlert size={13}/>完全访问</span><span>{user} · 虚拟机 ~/{product.terminal.working_directory}</span>
      {meter && <MeterView meter={meter}/>}
    </div>
  </div>;
}
