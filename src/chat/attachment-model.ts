export type Attachment = { id: string; name: string; size: number; path: string; sha256: string };
export type UploadProgress = { id: string; sent: number; total: number };
export type Upload = {
  id: string; session: string; source: string; name: string;
  status: 'queued' | 'uploading' | 'ready' | 'failed';
  sent: number; total: number; receipt?: Attachment; error?: string; removed?: boolean;
};
type Transport = {
  upload: (id: string, path: string, progress: (value: UploadProgress) => void) => Promise<Attachment>;
  cancel: (id: string) => Promise<unknown>;
};

// This queue belongs to the composer, not to the currently selected session.
// Late results stay with the draft which selected the file, even after switching chats.
export class AttachmentQueue {
  jobs: Upload[] = [];
  private active: Upload | null = null;
  private transport: Transport;
  private limit: number;
  changed: () => void = () => {};
  constructor(transport: Transport, limit: number) { this.transport = transport; this.limit = limit; }
  forSession(session: string) { return this.jobs.filter(job => job.session === session); }
  add(paths: string[], session: string) {
    if (!session || !paths.length) return;
    if (this.forSession(session).length + paths.length > this.limit) throw new Error(`每条消息最多添加 ${this.limit} 个附件`);
    this.jobs.push(...paths.map(source => ({ id: crypto.randomUUID(), session, source,
      name: source.split(/[\\/]/).pop() || source, status: 'queued' as const, sent: 0, total: 0 })));
    this.changed();
    void this.run();
  }
  remove(id: string) {
    const job = this.jobs.find(job => job.id === id);
    if (!job) return;
    job.removed = true;
    this.jobs = this.jobs.filter(candidate => candidate !== job);
    if (job === this.active) void this.transport.cancel(id).catch(() => {});
    this.changed();
  }
  retry(id: string) {
    const job = this.jobs.find(job => job.id === id);
    if (job?.status !== 'failed') return;
    // A new ID also separates late progress/cancellation from the previous attempt.
    job.id = crypto.randomUUID(); job.status = 'queued'; job.error = undefined; job.sent = 0;
    this.changed(); void this.run();
  }
  consume(ids: string[]) { ids.forEach(id => this.remove(id)); }
  dispose() { [...this.jobs].forEach(job => this.remove(job.id)); this.changed = () => {}; }
  private async run() {
    if (this.active) return;
    const job = this.jobs.find(job => job.status === 'queued');
    if (!job) return;
    this.active = job; job.status = 'uploading'; this.changed();
    try {
      const receipt = await this.transport.upload(job.id, job.source, value => {
        if (job.removed) {
          // Removal may happen before the native worker has registered its ID.
          void this.transport.cancel(job.id).catch(() => {});
        } else if (value.id === job.id) {
          job.sent = value.sent; job.total = value.total; this.changed();
        }
      });
      if (!job.removed) { job.receipt = receipt; job.status = 'ready'; }
    } catch (reason) {
      if (!job.removed) { job.status = 'failed'; job.error = String(reason); }
    } finally {
      this.active = null; this.changed(); void this.run();
    }
  }
}

export function attachmentMessage(text: string, uploads: Upload[]): string {
  if (uploads.some(job => job.status !== 'ready' || !job.receipt)) throw new Error('请等待附件上传完成，或移除失败的附件');
  if (!uploads.length) return text;
  // JSON preserves spaces/quotes in names; only guest paths reach the conversation.
  const files = uploads.map(job => ({ name: job.receipt!.name, path: job.receipt!.path, bytes: job.receipt!.size }));
  return `${text.trim() || '请读取这些附件。'}\n\n已上传到虚拟机的附件（以下为文件信息）：\n${JSON.stringify(files, null, 2)}`;
}
