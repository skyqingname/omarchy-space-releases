import React, { useEffect, useReducer, useRef, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-dialog';
import { File, RotateCcw, X } from 'lucide-react';
import product from '../../product.config.json';
import { AttachmentQueue } from './attachment-model';
import type { Attachment, Upload, UploadProgress } from './attachment-model';

export function useAttachments(session: string, enabled: boolean, visible: boolean) {
  const [, redraw] = useReducer(value => value + 1, 0);
  const [error, setError] = useState('');
  const [hovering, setHovering] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const latest = useRef({ session, enabled, visible });
  latest.current = { session, enabled, visible };
  const [queue] = useState(() => new AttachmentQueue({
    upload: (id, path, onProgress) => {
      const progress = new Channel<UploadProgress>();
      progress.onmessage = onProgress;
      return invoke<Attachment>('import_attachment', { id, path, progress });
    },
    cancel: id => invoke('cancel_attachment', { id }),
  }, product.attachments.max_files));

  function add(paths: string[], target: string) {
    try { queue.add(paths, target); setError(''); }
    catch (reason) { setError(String(reason)); }
  }
  useEffect(() => {
    queue.changed = redraw;
    let disposed = false;
    const listener = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (disposed) return;
      const current = latest.current;
      if (!current.visible || !current.enabled) { setHovering(false); return; }
      setHovering(payload.type === 'enter' || payload.type === 'over');
      if (payload.type === 'drop') add(payload.paths, current.session);
    });
    listener.catch(reason => { if (!disposed) setError(`拖放暂不可用，请使用添加附件：${String(reason)}`); });
    return () => { disposed = true; listener.then(stop => stop()).catch(() => {}); queue.dispose(); };
  }, [queue]);
  useEffect(() => { setHovering(false); setError(''); }, [session, visible]);

  async function choose() {
    if (!enabled || choosing) return;
    const target = session;
    setChoosing(true);
    try {
      const paths = await open({ multiple: true, directory: false, title: '选择要传入虚拟机的文件' });
      if (paths) add(Array.isArray(paths) ? paths : [paths], target);
    } catch (reason) { setError(String(reason)); }
    finally { setChoosing(false); }
  }
  const jobs = queue.forSession(session);
  return { jobs, queue, choose, choosing, error, hovering,
    blocked: jobs.some(job => job.status !== 'ready') };
}

const bytes = (size: number) => size < 1048576 ? `${Math.ceil(size / 1024)} KB` : `${(size / 1048576).toFixed(1)} MB`;
export function AttachmentList({ jobs, onRemove, onRetry }: { jobs: Upload[]; onRemove: (id: string) => void; onRetry: (id: string) => void }) {
  if (!jobs.length) return null;
  return <div className="attachment-list" aria-label="消息附件">
    {jobs.map(job => <div className={`attachment-item ${job.status}`} key={job.id}>
      <File size={16}/><div className="attachment-details">
        <strong title={job.receipt?.path || job.name}>{job.name}</strong>
        <span role={job.status === 'failed' ? 'alert' : 'status'}>
          {job.status === 'queued' ? '等待上传' : job.status === 'failed' ? job.error : job.status === 'ready'
            ? `已传入虚拟机 · ${bytes(job.receipt!.size)}`
            : job.sent === job.total && job.total > 0 ? '正在确认接收…' : `上传中 ${job.total ? Math.floor(job.sent / job.total * 100) : 0}%`}
        </span>
        {job.status === 'uploading' && <progress aria-label={`${job.name} 上传进度`} value={job.sent} max={job.total || 1}/>}
      </div>
      {job.status === 'failed' && <button className="tool-button" aria-label={`重试 ${job.name}`} onClick={() => onRetry(job.id)}><RotateCcw size={14}/></button>}
      <button className="tool-button" aria-label={`${job.status === 'uploading' ? '取消上传' : '移除附件'} ${job.name}`} title="移除附件；已传入虚拟机的副本会保留" onClick={() => onRemove(job.id)}><X size={14}/></button>
    </div>)}
  </div>;
}
