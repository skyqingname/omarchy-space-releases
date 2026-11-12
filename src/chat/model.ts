// What the chat knows of Claude Code: the items Rust reduces from its
// transcript, the hooks it ran and its status line, and what the page derives
// from them. Nothing here touches React or Tauri, so a recorded event stream
// can be run through it from Node.

export type Question = { question: string; header: string; multiSelect: boolean; options: { label: string; description: string }[] };
export type User = { kind: 'user'; id: string; at: string; text: string; images: number };
export type Assistant = { kind: 'assistant'; id: string; at: string; text: string };
export type Thinking = { kind: 'thinking'; id: string; at: string; text: string; ms: number | null };
export type Tool = { kind: 'tool'; id: string; at: string; tool_id: string; name: string; summary: string; detail: string | null; input: string; added: number; removed: number; questions: Question[] | null };
export type Result = { kind: 'tool_result'; id: string; at: string; tool_id: string; output: string; error: boolean };
export type TurnEnd = { kind: 'turn_end'; id: string; at: string; ms: number };
export type Interrupt = { kind: 'interrupt'; id: string; at: string };
export type Command = { kind: 'command'; id: string; at: string; text: string };
export type Notice = { kind: 'notice'; id: string; at: string; text: string };
export type Compact = { kind: 'compact'; id: string; at: string; trigger: 'auto' | 'manual' | null; tokens: number | null };
// Items with a place in the transcript.
export type Entry = User | Assistant | Thinking | Tool | Result | TurnEnd | Interrupt | Command | Notice | Compact;
export type Queue = { kind: 'queue'; op: 'enqueue'; text: string } | { kind: 'queue'; op: 'dequeue' | 'remove'; text: string | null };
export type Title = { kind: 'title'; text: string };
export type Hook = { kind: 'hook'; event: string; notification: string | null; tool: string | null };
export type Item = Entry | Queue | Title | Hook;

export type Status = { signed_in: boolean; onboarded: boolean; trusted: boolean };
export type Usage = { input_tokens: number; cache_creation_input_tokens: number; cache_read_input_tokens: number; output_tokens: number };
export type Limit = { used_percentage: number; resets_at: number };
export type Meter = {
  model?: { id: string; display_name: string };
  effort?: { level: string };
  context_window?: { context_window_size: number; used_percentage: number | null; current_usage: Usage | null };
  rate_limits?: { five_hour: Limit; seven_day: Limit };
};
export type ChatEvent = { generation: number; items: Item[]; reset?: true; session_id?: string; live?: number; status?: Status; meter?: Meter };

export type Activity = 'idle' | 'working' | 'waiting';
// A message sent from the chat that the transcript has not recorded yet.
export type Pending = { text: string; at: number; queued: boolean };
export type ChatState = {
  items: Entry[];
  // Where live activity begins in `items`; null while the followed session's history is still arriving.
  live: number | null;
  session: string;
  title: string;
  status: Status | null;
  meter: Meter | null;
  activity: Activity;
  // The tool the hook that last made Claude wait was about.
  asked: string | null;
  // Date.now() of the last change of `activity` and of the last live event, for the page's timers.
  changed: number;
  heard: number;
  // How far this computer's clock is ahead of the VM's, as the last live entry showed on arrival.
  // The VM's clock stands still while the computer sleeps, so running timers count on this one.
  skew: number;
  pending: Pending[];
  exited: boolean;
};
export type Action =
  | { type: 'event'; event: ChatEvent; now: number }
  | { type: 'send'; text: string; now: number }
  // The page interrupted Claude; hooks say so again if it goes on.
  | { type: 'stop'; now: number }
  | { type: 'exit'; now: number }
  | { type: 'clear' };

export const initial: ChatState = { items: [], live: null, session: '', title: '', status: null, meter: null, activity: 'idle', asked: null, changed: 0, heard: 0, skew: 0, pending: [], exited: false };

// The TUI keeps a pasted message's words but not its spacing: `!ls` is recorded as `! ls`.
const same = (a: string, b: string) => a.replace(/\s+/g, '') === b.replace(/\s+/g, '');

// What a hook says Claude is doing; null when it says nothing of that.
function hooked(hook: Hook): Activity | null {
  switch (hook.event) {
    case 'UserPromptSubmit': case 'PostToolUse': case 'PostToolUseFailure': return 'working';
    case 'PermissionRequest': return 'waiting';
    case 'Stop': case 'SessionStart': return 'idle';
    case 'Notification':
      if (hook.notification === 'permission_prompt' || hook.notification === 'elicitation_dialog') return 'waiting';
      return hook.notification === 'idle_prompt' ? 'idle' : null;
  }
  return null;
}

// A recorded prompt is the pending one with its words, and every one sent
// before it; with no such pending (index -1) all stay.
function echoed(pending: Pending[], text: string) {
  return pending.slice(pending.findIndex(item => same(item.text, text)) + 1);
}

function queued(pending: Pending[], text: string) {
  const at = pending.findIndex(item => !item.queued && same(item.text, text));
  return pending.map((item, index) => index === at ? { ...item, queued: true } : item);
}

function absorb(state: ChatState, event: ChatEvent, now: number): ChatState {
  // A reset follows another session: nothing said in the one before stays,
  // including what was sent there, which that session may never record.
  const reset = event.reset === true;
  const items = reset ? [] : [...state.items];
  let live = reset ? null : state.live;
  let title = reset ? '' : state.title;
  let pending = reset ? [] : state.pending;
  let activity: Activity = reset ? 'idle' : state.activity;
  let asked = reset ? null : state.asked;
  let { heard, skew } = state;
  for (const [index, item] of event.items.entries()) {
    if (index === event.live) live = items.length;
    if (live !== null) heard = now;
    if (item.kind === 'hook') {
      const told = hooked(item);
      if (told === 'waiting' && item.tool !== null) asked = item.tool;
      activity = told ?? activity;
    }
    else if (item.kind === 'title') title = item.text;
    else if (item.kind === 'queue') { if (item.op === 'enqueue') pending = queued(pending, item.text); }
    else {
      items.push(item);
      if (live === null) continue;
      skew = now - Date.parse(item.at);
      if (item.kind === 'interrupt') activity = 'idle';
      if (item.kind === 'user' || item.kind === 'command') pending = echoed(pending, item.text);
    }
  }
  if (event.live === event.items.length) live = items.length;
  return {
    ...state, items, live, title, pending, activity, asked, heard, skew,
    changed: activity === state.activity ? state.changed : now,
    session: event.session_id ?? state.session,
    status: event.status ?? state.status,
    meter: event.meter ?? state.meter,
  };
}

const settle = (state: ChatState, now: number): ChatState => state.activity === 'idle' ? state : { ...state, activity: 'idle', changed: now };

export function reduce(state: ChatState, action: Action): ChatState {
  switch (action.type) {
    case 'clear': return initial;
    case 'send': return { ...state, pending: [...state.pending, { text: action.text, at: action.now, queued: false }] };
    // The user gave up on what was sent as well: a command the TUI never records would wait forever.
    case 'stop': return { ...settle(state, action.now), pending: [] };
    case 'exit': return { ...settle(state, action.now), exited: true, pending: [] };
    case 'event': return absorb(state, action.event, action.now);
  }
}

export type Phase = 'exited' | 'waiting' | 'working' | 'idle';
// What the page says Claude is doing. A message just sent counts as work
// before Claude has taken it up.
export function phase(state: ChatState): Phase {
  if (state.exited) return 'exited';
  if (state.activity === 'waiting') return 'waiting';
  return state.activity === 'working' || state.pending.length > 0 ? 'working' : 'idle';
}

// Thinking and tool calls between two things said, folded into one line.
export type Work = { kind: 'work'; id: string; items: (Thinking | Tool)[] };
export type Step = Assistant | Work | Compact | Notice;
export type Turn = {
  id: string;
  question: User | Command | null;
  steps: Step[];
  // A finished turn's closing reply, taken out of `steps`.
  answer: Assistant | null;
  start: string;
  // Null only for the active turn.
  end: string | null;
  // The duration Claude reported.
  ms: number | null;
  interrupted: boolean;
  live: boolean;
  tools: number;
  // Replies before the answer.
  notes: number;
};
export type Conversation = { turns: Turn[]; results: Map<string, Result>; active: Turn | null };

const callsOf = (turn: Turn) => turn.steps.flatMap(step => step.kind === 'work' ? step.items : []).filter((item): item is Tool => item.kind === 'tool');

// One turn per question: a user message or command starts one, Claude's end
// of turn or an interruption closes it. The last turn is active while it is
// open, began live and Claude is at it; any other open turn ended with the
// last thing it took. Items keep joining an open turn either way: the Stop
// hook comes a moment before the answer and the end of turn it belongs to.
export function conversation(items: Entry[], live: number | null, activity: Activity): Conversation {
  const turns: Turn[] = [];
  const results = new Map<string, Result>();
  const begin = (question: User | Command | null, item: Entry, index: number) => {
    const turn: Turn = { id: item.id, question, steps: [], answer: null, start: item.at, end: null, ms: null, interrupted: false, live: live !== null && index >= live, tools: 0, notes: 0 };
    turns.push(turn);
    return turn;
  };
  // The turn that takes the next items, and when anything last happened.
  let open: Turn | null = null;
  let last = '';
  for (const [index, item] of items.entries()) {
    if (item.kind === 'tool_result') { results.set(item.tool_id, item); last = item.at; continue; }
    if (item.kind === 'turn_end' || item.kind === 'interrupt') {
      if (open === null) continue;
      open.end = item.at;
      open.ms = item.kind === 'turn_end' ? item.ms : null;
      open.interrupted = item.kind === 'interrupt';
      open = null;
      continue;
    }
    last = item.at;
    if (item.kind === 'user' || item.kind === 'command') {
      if (open !== null) open.end = item.at;
      open = begin(item, item, index);
      continue;
    }
    // What comes after a turn ended and before the next question is a turn of its own.
    open ??= begin(null, item, index);
    if (item.kind === 'thinking' || item.kind === 'tool') {
      const step = open.steps.at(-1);
      if (step?.kind === 'work') step.items.push(item);
      else open.steps.push({ kind: 'work', id: item.id, items: [item] });
      if (item.kind === 'tool') open.tools++;
    } else open.steps.push(item);
  }
  const final = turns.at(-1);
  const active = final !== undefined && final.end === null && final.live && activity !== 'idle' ? final : null;
  if (final !== undefined && final.end === null && active === null) final.end = last;
  for (const turn of turns) {
    const step = turn.steps.at(-1);
    if (turn !== active && step?.kind === 'assistant') { turn.answer = step; turn.steps.pop(); }
    turn.notes = turn.steps.filter(step => step.kind === 'assistant').length;
  }
  return { turns, results, active };
}

export type Category = 'command' | 'edit' | 'read' | 'search' | 'web' | 'agent' | 'other';
const categories: Record<string, Category> = {
  Bash: 'command', PowerShell: 'command', Edit: 'edit', MultiEdit: 'edit', Write: 'edit', NotebookEdit: 'edit',
  Read: 'read', Grep: 'search', Glob: 'search', WebFetch: 'web', WebSearch: 'web', Task: 'agent', Agent: 'agent',
};
// [verb while running, verb once done, what a running group is doing, what a finished group did]
const words: Record<Category, [string, string, string, (count: number) => string]> = {
  command: ['运行', '已运行', '运行命令', count => `运行了 ${count} 条命令`],
  edit: ['编辑', '已编辑', '编辑文件', count => `编辑了 ${count} 个文件`],
  read: ['读取', '已读取', '读取文件', count => `读取了 ${count} 个文件`],
  search: ['搜索', '已搜索', '搜索', count => `搜索了 ${count} 次`],
  web: ['检索', '已检索', '检索网页', count => `检索了 ${count} 个网页`],
  agent: ['子任务', '子任务完成', '执行子任务', count => `执行了 ${count} 个子任务`],
  other: ['调用', '已调用', '调用工具', count => `调用了 ${count} 个工具`],
};
// Tools the chat has no words of its own for are "other".
export const categoryOf = (tool: Tool): Category => categories[tool.name] ?? 'other';

// One line for a call: what it does, to what, and for an edit how many lines go in and out.
// `active` is whether its turn is the active one: only there does a call without a result run.
export function describe(tool: Tool, result: Result | undefined, active: boolean) {
  const category = categoryOf(tool);
  const running = active && result === undefined;
  const subject = category === 'command' ? tool.summary.split('\n')[0]
    : category === 'edit' || category === 'read' ? tool.summary.split(/[\\/]/).at(-1)!
    : category === 'other' ? `${tool.name} ${tool.summary}`.trim()
    : tool.summary;
  return {
    category, running, subject, title: tool.summary,
    verb: words[category][running ? 0 : 1],
    added: category === 'edit' ? tool.added : 0,
    removed: category === 'edit' ? tool.removed : 0,
    failed: result !== undefined && result.error,
  };
}

// The folded line of a work group: what runs now with the time it has run
// since `since`, or what was done and how long it took (`ms`).
export function summarize(work: Work, results: Map<string, Result>, active: boolean, latest: boolean) {
  const tools = work.items.filter((item): item is Tool => item.kind === 'tool');
  const start = work.items[0].at;
  const running = active ? tools.filter(tool => !results.has(tool.tool_id)) : [];
  if (running.some(tool => tool.name === 'AskUserQuestion')) return { label: '等你回答', running: true, since: null, ms: null };
  if (running.length) return { label: `正在${[...new Set(running.map(tool => words[categoryOf(tool)][2]))].join('、')}`, running: true, since: start, ms: null };
  if (!tools.length) {
    const timed = work.items.filter((item): item is Thinking => item.kind === 'thinking' && item.ms !== null);
    return { label: active && latest ? '正在思考' : '思考', running: active && latest, since: null, ms: timed.length ? timed.reduce((sum, item) => sum + item.ms!, 0) : null };
  }
  const counts = new Map<Category, number>();
  for (const tool of tools) counts.set(categoryOf(tool), (counts.get(categoryOf(tool)) ?? 0) + 1);
  // A call left without a result ended, as far as the group goes, where it began.
  const end = Math.max(...work.items.map(item => Date.parse(item.at)), ...tools.map(tool => Date.parse(results.get(tool.tool_id)?.at ?? tool.at)));
  return { label: [...counts].map(([category, count]) => words[category][3](count)).join('，'), running: false, since: null, ms: end - Date.parse(start) };
}

// What Claude waits on in the active turn: the last call without a result, a question before any other.
export function awaited(turn: Turn, results: Map<string, Result>): Tool | null {
  const open = callsOf(turn).filter(tool => !results.has(tool.tool_id)).reverse();
  return open.find(tool => tool.name === 'AskUserQuestion') ?? open.at(0) ?? null;
}

// The question the chat can answer with one key: a single choice among at most `limit` options.
export function choice(tool: Tool, limit: number): Question | null {
  if (tool.name !== 'AskUserQuestion' || tool.questions === null || tool.questions.length !== 1) return null;
  const [question] = tool.questions;
  return !question.multiSelect && question.options.length <= limit ? question : null;
}

// For the run indicator: since when the work is counted, when anything was
// last heard of it, and whether a call is at work (it tells its own time).
export function progress(state: ChatState, talk: Conversation) {
  return {
    since: talk.active ? Date.parse(talk.active.start) + state.skew : state.pending.length ? state.pending[0].at : state.changed,
    heard: state.pending.reduce((latest, item) => Math.max(latest, item.at), state.heard),
    calling: talk.active !== null && callsOf(talk.active).some(tool => !talk.results.has(tool.tool_id)),
  };
}

export type SlashCommand = { name: string; description: string; terminal: boolean };
// The command menu for a draft that is a slash and part of a name: names that start with it, then names that contain it.
export function slashMatches(draft: string, commands: SlashCommand[]) {
  if (!/^\/\S*$/.test(draft)) return [];
  const typed = draft.slice(1).toLowerCase();
  return [...commands.filter(command => command.name.startsWith(typed)), ...commands.filter(command => !command.name.startsWith(typed) && command.name.includes(typed))];
}

// Whether a message is a command that opens a dialog in the TUI.
export function opensDialog(text: string, commands: SlashCommand[]) {
  const word = text.split(/\s/, 1)[0];
  return commands.some(command => command.terminal && `/${command.name}` === word);
}

// The context window holds the prompt, as read from cache or written to it.
export const usedTokens = (usage: Usage) => usage.input_tokens + usage.cache_creation_input_tokens + usage.cache_read_input_tokens;
