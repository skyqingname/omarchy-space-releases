// Times and durations as the chat words them.

export function duration(ms: number) {
  // A live timer compares the VM's clock with this computer's, which may run a little ahead.
  const exact = Math.max(0, ms);
  const seconds = Math.floor(exact / 1000);
  if (seconds < 10) return `${(exact / 1000).toFixed(1)} 秒`;
  if (seconds < 60) return `${seconds} 秒`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒`;
  return `${Math.floor(seconds / 3600)} 小时 ${Math.floor((seconds % 3600) / 60)} 分`;
}

// Making a formatter costs far more than using one, and every message asks for its time.
const clock = new Intl.DateTimeFormat('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false });
const day = new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric' });
const dayOfYear = new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'numeric', day: 'numeric' });
const whole = new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });

function dayOf(date: Date, now: Date) {
  if (date.toDateString() === now.toDateString()) return '今天';
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (date.toDateString() === yesterday.toDateString()) return '昨天';
  return (date.getFullYear() === now.getFullYear() ? day : dayOfYear).format(date);
}

// "今天 22:08" under a message, the whole timestamp in its tip.
export function messageTime(at: string) {
  const date = new Date(at);
  return { short: `${dayOf(date, new Date())} ${clock.format(date)}`, full: whole.format(date) };
}

// How long ago a session was last changed, from seconds since the epoch.
export function ago(seconds: number) {
  const date = new Date(seconds * 1000);
  const now = new Date();
  const passed = (now.getTime() - date.getTime()) / 1000;
  if (passed < 60) return '刚刚';
  if (passed < 3600) return `${Math.floor(passed / 60)} 分钟前`;
  const label = dayOf(date, now);
  return label === '今天' ? `${Math.floor(passed / 3600)} 小时前` : label;
}

// When a usage limit resets, from seconds since the epoch, in local time.
export function resetTime(seconds: number) {
  const date = new Date(seconds * 1000);
  const label = date.toDateString() === new Date().toDateString() ? '今天' : day.format(date);
  return `${label} ${clock.format(date)}`;
}
