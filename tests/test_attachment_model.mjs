import assert from 'node:assert/strict';
import { test } from 'node:test';
import { AttachmentQueue, attachmentMessage } from '../src/chat/attachment-model.ts';

const tick = () => new Promise(resolve => setImmediate(resolve));
const receipt = (id, source) => ({ id, name: source.split(/[\\/]/).pop(), size: 42,
  path: `/home/guest/workspace/attachments/${id}/file.pdf`, sha256: 'a'.repeat(64) });
function fixture() {
  const calls = [], cancellations = [];
  const queue = new AttachmentQueue({
    upload: (id, path, progress) => new Promise((resolve, reject) => calls.push({ id, path, progress, resolve, reject })),
    cancel: async id => { cancellations.push(id); },
  }, 2);
  return { queue, calls, cancellations };
}

test('serial uploads keep late results in their originating session', async () => {
  const { queue, calls } = fixture();
  queue.add(['C:\\文件\\报告.pdf', 'D:\\second.docx'], 'one');
  queue.add(['D:\\third.txt'], 'two');
  assert.equal(calls.length, 1);
  calls[0].progress({ id: calls[0].id, sent: 21, total: 42 });
  assert.equal(queue.forSession('one')[0].sent, 21);
  calls[0].resolve(receipt(calls[0].id, calls[0].path)); await tick();
  assert.equal(queue.forSession('one')[0].status, 'ready');
  assert.equal(queue.forSession('two')[0].status, 'queued');
  assert.equal(calls.length, 2);
  calls[1].resolve(receipt(calls[1].id, calls[1].path)); await tick();
  assert.equal(calls.length, 3);
  calls[2].resolve(receipt(calls[2].id, calls[2].path)); await tick();
});

test('remove before native registration cancels again when progress arrives', async () => {
  const { queue, calls, cancellations } = fixture();
  queue.add(['C:\\file.pdf'], 'one');
  const id = calls[0].id;
  queue.remove(id);
  calls[0].progress({ id, sent: 0, total: 42 });
  calls[0].resolve(receipt(id, 'file.pdf')); await tick();
  assert.deepEqual(cancellations, [id, id]);
  assert.equal(queue.jobs.length, 0);
});

test('failed uploads cannot be sent; retry uses a fresh ID', async () => {
  const { queue, calls } = fixture();
  queue.add(['C:\\file.pdf'], 'one');
  calls[0].reject('connection lost'); await tick();
  assert.equal(queue.jobs[0].status, 'failed');
  assert.throws(() => attachmentMessage('read it', queue.jobs), /上传/);
  queue.retry(calls[0].id);
  assert.equal(calls.length, 2); assert.notEqual(calls[0].id, calls[1].id);
  calls[1].resolve(receipt(calls[1].id, calls[1].path)); await tick();
  assert.equal(queue.jobs[0].status, 'ready');
});

test('batch limit rejects the entire selection without dropping existing attachments', async () => {
  const { queue, calls } = fixture();
  queue.add(['a'], 'one');
  assert.throws(() => queue.add(['b', 'c'], 'one'), /最多/);
  assert.equal(queue.jobs.length, 1);
  calls[0].resolve(receipt(calls[0].id, calls[0].path)); await tick();
});

test('message includes only guest paths and consumes only the sent snapshot', async () => {
  const { queue, calls } = fixture();
  queue.add(['C:\\private\\report.pdf'], 'one');
  calls[0].resolve(receipt(calls[0].id, calls[0].path)); await tick();
  const snapshot = [...queue.jobs];
  const message = attachmentMessage('', snapshot);
  assert.match(message, /请读取这些附件/);
  assert.match(message, /\/home\/guest\/workspace\/attachments/);
  assert.ok(!message.includes('C:') && !message.includes('private'));
  queue.add(['D:\\next.docx'], 'one');
  queue.consume(snapshot.map(job => job.id));
  assert.equal(queue.jobs.length, 1);
  calls[1].resolve(receipt(calls[1].id, calls[1].path)); await tick();
});

test('dispose cancels the active upload and never starts queued work', async () => {
  const { queue, calls, cancellations } = fixture();
  queue.add(['a', 'b'], 'one');
  queue.dispose(); calls[0].reject('cancelled'); await tick();
  assert.equal(calls.length, 1); assert.equal(queue.jobs.length, 0);
  assert.deepEqual(cancellations, [calls[0].id]);
});
