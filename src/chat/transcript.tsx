import React, { memo, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Marked, Renderer } from 'marked';
import { ArrowDown, Bot, Brain, Check, ChevronRight, Copy, FilePen, FileText, Globe, Image, LoaderCircle, Search, SquareTerminal, Wrench } from 'lucide-react';
import product from '../../product.config.json';
import { awaited, choice, describe, progress, summarize } from './model';
import type { Assistant, Category, ChatState, Conversation, Phase, Question, Result, Step, Thinking, Tool, Turn, User, Work } from './model';
import { duration, messageTime } from './format';

const escape = (text: string) => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const plain = new Renderer();
const markdown = new Marked({
  renderer: {
    // Markup in a reply is shown as the text it is.
    html({ text, block }) { return block ? `<p>${escape(text)}</p>` : escape(text); },
    // A picture would be fetched by this computer, outside the VM's node: it stays a link.
    image({ href, text }) { return `<a href="${escape(href)}">${escape(text || href)}</a>`; },
    // The transcript handles the button's clicks, as it does the links'.
    code(token) { return `<div class="code-block">${plain.code(token)}<button type="button" class="code-copy">复制</button></div>`; },
  },
});

// Drawn again only when its text changes, not with every tick of a timer.
const Markdown = memo(function Markdown({ text }: { text: string }) {
  return <div className="markdown" dangerouslySetInnerHTML={{ __html: markdown.parse(text, { async: false }) }}/>;
});

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), product.chat.copy_feedback_milliseconds);
    return () => window.clearTimeout(timer);
  }, [copied]);
  return <button type="button" className="copy-button" aria-label="复制" title={copied ? '已复制' : '复制'} onClick={() => navigator.clipboard.writeText(text).then(() => setCopied(true))}>
    {copied ? <Check size={13}/> : <Copy size={13}/>}
  </button>;
}

const icons: Record<Category, typeof Wrench> = { command: SquareTerminal, edit: FilePen, read: FileText, search: Search, web: Globe, agent: Bot, other: Wrench };

function Block({ text, failed = false }: { text: string; failed?: boolean }) {
  return <div className="tool-block"><pre className={failed ? 'failed' : ''}>{text}</pre><CopyButton text={text}/></div>;
}

function ToolRow({ tool, result, active, now }: { tool: Tool; result: Result | undefined; active: boolean; now: number }) {
  const [open, setOpen] = useState(false);
  const { category, verb, subject, title, added, removed, running, failed } = describe(tool, result, active);
  const Icon = icons[category];
  const time = result ? duration(Date.parse(result.at) - Date.parse(tool.at)) : running ? duration(now - Date.parse(tool.at)) : '';
  return <div className={`tool-row ${failed ? 'failed' : ''}`}>
    <button className="tool-call" onClick={() => setOpen(!open)}>
      {running ? <LoaderCircle className="spin" size={14}/> : <Icon size={14}/>}
      <b>{verb}</b>
      <code title={title}>{subject}</code>
      {(added > 0 || removed > 0) && <span className="tool-diff"><ins>+{added}</ins><del>−{removed}</del></span>}
      <span className="tool-time">{time}</span>
    </button>
    {open && <div className="tool-detail">
      {tool.detail && <Block text={tool.detail}/>}
      <Block text={tool.input}/>
      {result?.output && <Block text={result.output} failed={result.error}/>}
    </div>}
  </div>;
}

function ThinkingRow({ item }: { item: Thinking }) {
  const [open, setOpen] = useState(false);
  const label = <><Brain size={14}/><b>思考{item.ms !== null && ` · ${duration(item.ms)}`}</b></>;
  if (!item.text) return <div className="tool-row"><span className="tool-call">{label}</span></div>;
  return <div className="tool-row">
    <button className="tool-call" onClick={() => setOpen(!open)}>{label}</button>
    {open && <div className="tool-detail thinking"><Markdown text={item.text}/></div>}
  </div>;
}

function WorkGroup({ work, results, active, latest, now }: { work: Work; results: Map<string, Result>; active: boolean; latest: boolean; now: number }) {
  const [open, setOpen] = useState(false);
  const { label, running, since, ms } = summarize(work, results, active, latest);
  const time = since !== null ? duration(now - Date.parse(since)) : ms !== null ? duration(ms) : '';
  return <div className="tool-group">
    <button className="tool-head" onClick={() => setOpen(!open)}>
      {running ? <LoaderCircle className="spin" size={14}/> : work.items.every(item => item.kind === 'thinking') ? <Brain size={14}/> : <Check size={14}/>}
      <span className="tool-title">{label}</span>
      {time && <small>{time}</small>}
      <ChevronRight size={14} className={open ? 'chevron open' : 'chevron'}/>
    </button>
    {open && <div className="tool-list">{work.items.map(item => item.kind === 'tool'
      ? <ToolRow key={item.id} tool={item} result={results.get(item.tool_id)} active={active} now={now}/>
      : <ThinkingRow key={item.id} item={item}/>)}</div>}
  </div>;
}

function StepView({ step, results, active, latest, now }: { step: Step; results: Map<string, Result>; active: boolean; latest: boolean; now: number }) {
  switch (step.kind) {
    case 'assistant': return <div className="message assistant"><Markdown text={step.text}/></div>;
    case 'work': return <WorkGroup work={step} results={results} active={active} latest={latest} now={now}/>;
    case 'compact': return <div className="compact-mark">{step.trigger === 'auto' ? '上下文已自动压缩' : '上下文已压缩'}{step.tokens !== null && ` · 压缩前 ${step.tokens.toLocaleString('zh-CN')} tokens`}</div>;
    case 'notice': return <div className="message notice"><pre>{step.text}</pre></div>;
  }
}

function Stamp({ at }: { at: string }) {
  const time = messageTime(at);
  return <time dateTime={at} title={time.full}>{time.short}</time>;
}

function UserMessage({ item }: { item: User }) {
  return <div className="message user">
    <div className="bubble">{item.text}{item.images > 0 && <span className="attachment"><Image size={13}/>{item.images} 张图片</span>}</div>
    <div className="message-footer"><Stamp at={item.at}/><CopyButton text={item.text}/></div>
  </div>;
}

function Answer({ item, spent, latest }: { item: Assistant; spent: string; latest: boolean }) {
  return <div className={`message assistant ${latest ? 'latest' : ''}`}>
    <Markdown text={item.text}/>
    <div className="message-footer"><CopyButton text={item.text}/><span>用时 {spent}</span><Stamp at={item.at}/></div>
  </div>;
}

// What a finished turn did between its question and its answer, folded into one line.
function TurnFold({ label, children }: { label: string; children: () => React.ReactNode }) {
  const [open, setOpen] = useState(false);
  return <div className="turn-fold">
    <button className="tool-head" onClick={() => setOpen(!open)}><span className="tool-title">{label}</span><ChevronRight size={14} className={open ? 'chevron open' : 'chevron'}/></button>
    {open && <div className="tool-list">{children()}</div>}
  </div>;
}

// Only the active turn has timers; the others are not drawn again as they tick.
// Its `now` is on the VM's clock, as its items' times are.
const TurnView = memo(function TurnView({ turn, results, active, latest, now }: { turn: Turn; results: Map<string, Result>; active: boolean; latest: boolean; now: number }) {
  const steps = () => turn.steps.map((step, index) => <StepView key={step.id} step={step} results={results} active={active} latest={index === turn.steps.length - 1} now={now}/>);
  const question = turn.question?.kind === 'user' ? <UserMessage item={turn.question}/>
    : turn.question?.kind === 'command' ? <div className="message command"><code>{turn.question.text}</code></div> : null;
  if (active) return <>{question}{steps()}</>;
  const spent = duration(turn.ms ?? Date.parse(turn.end!) - Date.parse(turn.start));
  // A command's output or a compaction, with nothing worked out around it, stays in view.
  const folds = turn.steps.some(step => step.kind === 'work' || step.kind === 'assistant');
  const label = `${turn.interrupted ? '已中断 · ' : ''}用时 ${spent}${turn.tools ? ` · ${turn.tools} 步` : ''}${turn.notes ? ` · ${turn.notes} 条说明` : ''}`;
  return <>
    {question}
    {folds ? <TurnFold label={label}>{steps}</TurnFold> : steps()}
    {!folds && turn.interrupted && <div className="message notice">{label}</div>}
    {turn.answer && <Answer item={turn.answer} spent={spent} latest={latest}/>}
  </>;
});

// Claude's question, when one key in the TUI answers it.
function QuestionCard({ question, onAnswer, onTerminal }: { question: Question; onAnswer: (index: number) => void; onTerminal: () => void }) {
  // One key answers; a second would land in whatever the TUI shows next.
  const [chosen, setChosen] = useState(false);
  return <div className="question-card" role="group" aria-label="Claude 的提问">
    <div className="question-head">{question.header && <span className="question-header">{question.header}</span>}<b>{question.question}</b></div>
    {question.options.map((option, index) => <button key={index} className="question-option" disabled={chosen} onClick={() => { setChosen(true); onAnswer(index); }}>
      <span className="question-number">{index + 1}</span><span><b>{option.label}</b><small>{option.description}</small></span>
    </button>)}
    <button className="text-button" onClick={onTerminal}>到终端回答 <SquareTerminal size={14}/></button>
  </div>;
}

function Waiting({ talk, asked, onAnswer, onTerminal }: { talk: Conversation; asked: string | null; onAnswer: (index: number) => void; onTerminal: () => void }) {
  const tool = talk.active && awaited(talk.active, talk.results);
  const question = tool && choice(tool, product.chat.question_options_max);
  if (tool && question) return <QuestionCard key={tool.id} question={question} onAnswer={onAnswer} onTerminal={onTerminal}/>;
  // The hook tells of a question a moment before the transcript holds it.
  if (!tool && asked === 'AskUserQuestion') return null;
  return <div className="tool-question"><span>等你回答{tool && `：${tool.summary}`}</span><button className="text-button" onClick={onTerminal}>到终端回答 <SquareTerminal size={14}/></button></div>;
}

function RunIndicator({ state, talk, now }: { state: ChatState; talk: Conversation; now: number }) {
  const { since, heard, calling } = progress(state, talk);
  const quiet = (now - heard) / 1000;
  // A call at work counts its own time; with none, a long silence is told as such.
  const silent = !calling && quiet >= product.chat.silent_seconds;
  return <div className="run-indicator" role="status">
    <span className={silent ? 'run-dot' : 'run-dot breathing'}/>
    {silent ? `仍在运行 · 已 ${Math.floor(quiet / 60)} 分钟没有新输出` : `正在工作 · ${duration(now - since)}`}
  </div>;
}

type Props = {
  state: ChatState; talk: Conversation; phase: Phase; now: number;
  onTerminal: () => void; onAnswer: (index: number) => void; onLink: (address: string) => void;
  // What follows the transcript, such as the row offered once Claude exited.
  children: React.ReactNode;
};

export function Transcript({ state, talk, phase, now, onTerminal, onAnswer, onLink, children }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const column = useRef<HTMLDivElement>(null);
  // The view follows new output until the reader moves up, and again once they come back down to the end.
  const follow = useRef(true);
  const lastTop = useRef(0);
  const sent = useRef(0);
  const [away, setAway] = useState(false);
  const toEnd = () => {
    const element = box.current!;
    element.scrollTop = element.scrollHeight;
    lastTop.current = element.scrollTop;
  };
  const rejoin = () => { follow.current = true; setAway(false); toEnd(); };
  const leave = () => { follow.current = false; setAway(true); };
  // Heights change with nothing new said: replies drawn, folds opened, the window resized.
  useEffect(() => {
    const observer = new ResizeObserver(() => { if (follow.current) toEnd(); });
    observer.observe(box.current!);
    observer.observe(column.current!);
    return () => observer.disconnect();
  }, []);
  // Another session, or a message just sent, is read from the end.
  useLayoutEffect(rejoin, [state.session]);
  useLayoutEffect(() => {
    if (state.pending.length > sent.current) rejoin();
    sent.current = state.pending.length;
  }, [state.pending.length]);

  function scrolled() {
    const element = box.current!;
    const top = element.scrollTop;
    const end = element.scrollHeight - top - element.clientHeight <= product.chat.scroll_end_pixels;
    if (end && top > lastTop.current && !follow.current) { follow.current = true; setAway(false); }
    else if (!end && top < lastTop.current && follow.current) leave();
    lastTop.current = top;
  }
  function clicked(event: React.MouseEvent) {
    const target = event.target as HTMLElement;
    const copy = target.closest<HTMLButtonElement>('.code-copy');
    if (copy) {
      navigator.clipboard.writeText(copy.previousElementSibling!.textContent!).then(() => {
        copy.textContent = '已复制';
        window.setTimeout(() => { copy.textContent = '复制'; }, product.chat.copy_feedback_milliseconds);
      });
      return;
    }
    const anchor = target.closest('a');
    if (!anchor) return;
    // The window itself never navigates; web links open in the VM's browser.
    event.preventDefault();
    const address = anchor.getAttribute('href')!;
    const scheme = /^([a-z][a-z\d+.-]*):/i.exec(address)?.[1].toLowerCase();
    if (scheme && product.terminal.link_schemes.includes(scheme)) onLink(address);
  }

  const { turns, results, active } = talk;
  return <div className="transcript-area">
    <div className="transcript" ref={box} onScroll={scrolled} onClick={clicked}
      onWheel={event => { if (event.deltaY < 0 && box.current!.scrollHeight > box.current!.clientHeight) leave(); }}>
      <div className="transcript-column" ref={column}>
        {turns.length === 0 && state.pending.length === 0 && <div className="chat-empty"><SquareTerminal size={28}/><b>Claude Code 在虚拟机里待命</b><p>直接输入要做的事。命令、文件改动都在虚拟机的独立磁盘里进行。<br/>需要看原始输出时，切到上方的「终端」。</p></div>}
        {turns.map((turn, index) => <TurnView key={turn.id} turn={turn} results={results} active={turn === active} latest={index === turns.length - 1} now={turn === active ? now - state.skew : 0}/>)}
        {phase === 'waiting' && <Waiting talk={talk} asked={state.asked} onAnswer={onAnswer} onTerminal={onTerminal}/>}
        {state.pending.map(item => <div className="message user pending" key={item.at}>
          <div className="bubble">{item.text}</div>
          <span className="pending-label">{item.queued ? '排队中，当前任务结束后处理' : '发送中'}</span>
        </div>)}
        {phase === 'working' && <RunIndicator state={state} talk={talk} now={now}/>}
        {children}
      </div>
    </div>
    {away && <button className="to-end" aria-label="回到底部" title="回到底部" onClick={rejoin}><ArrowDown size={16}/></button>}
  </div>;
}
