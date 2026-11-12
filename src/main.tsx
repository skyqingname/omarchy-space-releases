import React, { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { ArrowRight, ArrowUpRight, Check, ChevronLeft, Cpu, Disc3, FolderOpen, Globe, Globe2, HardDrive, Keyboard, KeyRound, LoaderCircle, LockKeyhole, MessageSquare, Monitor, Network, Play, Power, RefreshCw, Terminal, X } from 'lucide-react';
import { ChatPane } from './chat';
import { BrowserPane } from './browser';
import product from '../product.config.json';
import './style.css';

type Resources = { cpus: number; memory_mib: number; disk_gib: number };
type Region = {country:string;subdivision:string|null;timezone:string;timezone_mode:string;locale:string;browser_language:string;preferred_languages:string[]};
type Delivery = {host_access_active?:boolean;terminal_user?:string;claude_version:string;sing_box_version:string;egress_country:string;region:Region;proxy_required:boolean;network_guard_active:boolean};
// Rust owns this record. Which page is showing is derived from it, never stored.
type View = { phase: string; message: string; running: boolean; task: 'install'|'reconfigure'|'start'|'download'|null; environment: 'none'|'incomplete'|'restart_required'|'ready'; delivery: Delivery|null; failure: {kind:string;detail:string}|null; progress: number|null; host_access: boolean };
type Host = { platform: string; supported: boolean; cpu_count: number; hardware: {memory_mib:number;free_disk_gib:number;virtualization_ready:boolean;prerequisite:string}; assets: {ready: boolean; image_ready: boolean; download_bytes?: number; message: string; image_version?: string}; asset_root: string; data_directory: string; storage: {default_directory:string;can_change:boolean;disk_limit_gib:number}; runtime_profile: {min_disk_gib:number}; graphics:{available:boolean;software_rendering:boolean}; config: typeof product; view: View };
type Node = { index: number; name: string; protocol: string };
type Import = { nodes: Node[]; notices: string[] };
const steps = ['了解环境', '连接网络', '分配资源', '创建环境'];
const stages = [
  {id:'image',label:'创建独立磁盘',detail:'从固定版本的 Omarchy 镜像创建'},
  {id:'boot',label:'启动 Omarchy',detail:'打开虚拟机，建立配置通道'},
  {id:'network_install',label:'优先启动代理',detail:'启用断网保护，再启动 sing-box 与所选节点'},
  {id:'account',label:'完成首次设置',detail:'在 Omarchy 窗口里创建你的账号'},
  {id:'region',label:'匹配出口地区',detail:'设置系统与浏览器语言，固定出口时区'},
  {id:'claude_install',label:'设置 Claude Code',detail:'通过官方脚本安装，节点较慢时需要几分钟'},
  {id:'verify',label:'核验交付结果',detail:'检查服务、默认 Agent 和网络出口'},
];

function App() {
  const [host,setHost] = useState<Host|null>(null);
  const [view,setView] = useState<View|null>(null);
  const [step,setStep] = useState(0);
  const [page,setPage] = useState<'chat'|'browser'|'home'|'network'|'shortcuts'>('home');
  const [input,setInput] = useState('');
  const [subscription,setSubscription] = useState<Import|null>(null);
  const [selected,setSelected] = useState<number|null>(null);
  const [resources,setResources] = useState<Resources>(product.defaults);
  const [busy,setBusy] = useState(false);
  const [hostAccessPending,setHostAccessPending] = useState(false);
  const [error,setError] = useState('');
  const hostRequest = useRef(0);
  async function refreshHost(resetView = false) {
    const request = ++hostRequest.current;
    try {
      const result = await invoke<Host>('inspect_host');
      if (request === hostRequest.current) { setHost(result); if (resetView) setView(result.view); }
    } catch (error) { if (request === hostRequest.current) throw error; }
  }
  useEffect(() => {
    const unlisten = listen<View>('deployment', ({payload}) => {
      setView(payload);
      if (payload.phase === 'restart_required') { setSubscription(null); setSelected(null); }
      if (payload.phase === 'downloaded') refreshHost().catch(e=>setError(String(e)));
    });
    refreshHost(true).catch(e=>setError(String(e)));
    return () => { unlisten.then(fn=>fn()); };
  }, []);
  const configuring = view?.task === 'install' || view?.task === 'reconfigure';
  // The chat exists only while the started environment is ready.
  const chatUser = view?.running && view.environment === 'ready' && view.task === null && view.phase !== 'stopping' ? view.delivery?.terminal_user : undefined;
  const workspacePage = (page: string) => page === 'chat' || page === 'browser';
  useEffect(()=>{ setPage(current=>chatUser ? 'chat' : workspacePage(current) ? 'home' : current); },[chatUser]);
  const screen = configuring ? 'progress' : workspacePage(page) && !chatUser ? 'home' : page !== 'home' ? page : view?.environment === 'none' ? (['welcome','connection','resources'] as const)[step] : 'home';
  useEffect(()=>{window.scrollTo(0,0);},[screen]);
  useEffect(()=>{if(error) window.scrollTo(0,0);},[error]);
  useEffect(()=>{
    if (host) setResources(current=>({...current,disk_gib:Math.max(host.runtime_profile.min_disk_gib, Math.min(current.disk_gib,host.storage.disk_limit_gib))}));
  },[host?.storage.disk_limit_gib,host?.runtime_profile.min_disk_gib]);
  if (!host || !view) return <div className="app-shell"><aside className="sidebar"/><main><div className="main-content">{error&&<div role="alert" className="error-banner"><span>{error}</span></div>}</div></main></div>;
  async function perform(action:()=>Promise<unknown>) {
    setError(''); setBusy(true);
    try { await action(); } catch(e) { setError(String(e)); } finally { setBusy(false); }
  }
  const chooseAssets = () => perform(async()=>{
    const path = await open({directory:true,multiple:false,title:'选择离线资源包文件夹'});
    if (path) { await invoke('select_asset_pack',{path}); await refreshHost(); }
  });
  const chooseStorage = () => perform(async()=>{
    const path = await open({directory:true,multiple:false,defaultPath:host.data_directory,title:'选择虚拟机和镜像的存放位置'});
    if (path) { await invoke('select_storage_directory',{path}); await refreshHost(true); }
  });
  const download = () => perform(()=>invoke('download_image'));
  const importNodes = () => perform(async()=>{
    const result = await invoke<Import>('import_subscription',{input});
    setSubscription(result); setSelected(result.nodes[0].index); setInput('');
  });
  const apply = () => perform(async()=>{
    if (view.running) await invoke('stop_vm');
    await invoke('start_install',{nodeIndex:selected,resources});
    setPage('home');
  });
  const start = () => perform(()=>invoke('start_vm'));
  const stop = () => perform(()=>invoke('stop_vm'));
  const changeSoftwareRendering = (enabled:boolean) => perform(async()=>{
    await invoke('set_software_rendering',{enabled}); await refreshHost();
  });
  const restart = () => perform(async()=>{ await invoke('stop_vm'); await invoke('start_vm'); });
  const showDesktop = () => perform(()=>invoke('show_desktop'));
  const changeHostAccess = (enabled:boolean) => perform(async()=>{
    setHostAccessPending(enabled);
    try { await invoke('set_host_access',{enabled}); } finally { setHostAccessPending(false); }
  });
  const firstRun = view.environment === 'none' || view.task === 'install';
  const currentStep = configuring ? 3 : step;
  const navLocked = busy || view.task !== null || view.phase === 'stopping';
  const diskFits = resources.disk_gib >= host.runtime_profile.min_disk_gib && resources.disk_gib <= host.storage.disk_limit_gib && host.hardware.free_disk_gib >= product.limits.min_free_disk_gib;
  const alert = error || (view.phase === 'failed' ? view.message : '');
  const trail = firstRun ? ['创建工作环境', steps[currentStep]] : screen === 'chat' ? ['对话'] : screen === 'browser' ? ['浏览器'] : screen === 'network' ? ['我的环境', '网络节点'] : screen === 'shortcuts' ? ['我的环境', '快捷键'] : configuring ? ['我的环境', '正在配置'] : ['我的环境'];
  const picker = {input,setInput,subscription,selected,setSelected,busy,onImport:importNodes};
  const platformName = host.platform === 'macos-aarch64' ? 'Mac · Apple Silicon' : host.platform === 'windows-x86_64' ? 'Windows · x86_64' : host.platform;
  return <div className="app-shell">
    <aside className="sidebar">
      <div className="brand"><div className="brand-mark"><i/><i/><i/></div><span>{product.name.toUpperCase()}<span className="brand-sub">{product.full_name}</span></span></div>
      <div className="side-heading">一个环境。专注一件事。</div>
      {firstRun
        ?<nav aria-label="安装步骤">{steps.map((label,index)=><div key={label} className={`nav-step ${currentStep===index?'active':''} ${currentStep>index?'passed':''}`}><span className="step-number">{currentStep>index?<Check size={14}/>:String(index+1).padStart(2,'0')}</span><span>{label}</span>{currentStep===index&&<span className="active-dot"/>}</div>)}</nav>
        :<nav aria-label="环境导航">
          <button className={`nav-step ${screen==='chat'?'active':''}`} disabled={!chatUser} onClick={()=>setPage('chat')}><span className="step-number"><MessageSquare size={13}/></span><span>对话</span>{screen==='chat'&&<span className="active-dot"/>}</button>
          <button className={`nav-step ${screen==='browser'?'active':''}`} disabled={!chatUser} onClick={()=>setPage('browser')}><span className="step-number"><Globe size={13}/></span><span>浏览器</span>{screen==='browser'&&<span className="active-dot"/>}</button>
          <button className={`nav-step ${screen==='home'?'active':''}`} disabled={configuring} onClick={()=>setPage('home')}><span className="step-number"><Monitor size={13}/></span><span>我的环境</span>{screen==='home'&&<span className="active-dot"/>}</button>
          <button className={`nav-step ${screen==='network'||configuring?'active':''}`} disabled={navLocked} onClick={()=>setPage('network')}><span className="step-number"><Network size={13}/></span><span>网络节点</span>{(screen==='network'||configuring)&&<span className="active-dot"/>}</button>
          <button className={`nav-step ${screen==='shortcuts'?'active':''}`} disabled={configuring} onClick={()=>setPage('shortcuts')}><span className="step-number"><Keyboard size={13}/></span><span>快捷键</span>{screen==='shortcuts'&&<span className="active-dot"/>}</button>
        </nav>}
      <div className="side-bottom"><div className="side-note"><span className="status-dot"/> LOCAL WORKSPACE</div><p>你的系统，保持原样。<br/>新的工作，从这里开始。</p><div className="edition">DESKTOP / {product.version}</div></div>
    </aside>
    <main className={workspacePage(screen)?'chat-main':''}>
      <header><div className="breadcrumb">{trail.map((part,index)=><React.Fragment key={part}>{index>0&&<span>/</span>}{part}</React.Fragment>)}</div><span className="platform"><Monitor size={14}/>{platformName}</span></header>
      {chatUser&&<BrowserPane visible={screen==='browser'} platform={host.platform}/>}
      {chatUser&&<ChatPane visible={screen==='chat'} user={chatUser} onBrowser={()=>setPage('browser')} alert={alert&&<div role="alert" className="error-banner chat-error"><span>{alert}</span>{error&&<button aria-label="关闭提示" onClick={()=>setError('')}><X size={16}/></button>}</div>}/>}
      {!workspacePage(screen)&&<div className="main-content">
        {alert&&<div role="alert" className="error-banner"><span>{alert}</span>{error&&<button aria-label="关闭提示" onClick={()=>setError('')}><X size={16}/></button>}</div>}
        {screen==='welcome'&&<Welcome host={host} view={view} busy={navLocked} onChooseAssets={chooseAssets} onChooseStorage={chooseStorage} onDownload={download} onNext={()=>setStep(1)}/>}
        {screen==='connection'&&<>
          <div className="eyebrow">01 / CONNECTION</div><h1 className="compact">带上你的网络。</h1><p className="intro">粘贴订阅链接或节点链接。<br/>我们将它转换成 sing-box 配置，安装到新环境中。</p>
          <NetworkPicker {...picker}/>
          <div className="quiet-note"><LockKeyhole size={16}/><p>链接与凭据不写入宿主日志。客机保存配置时使用 systemd 凭据加密；Claude 登录在客机内完成。</p></div>
          <footer><button className="back" onClick={()=>setStep(0)}><ChevronLeft size={16}/>上一步</button><button className="primary" disabled={selected===null||busy} onClick={()=>setStep(2)}>分配资源 <ArrowRight size={17}/></button></footer>
        </>}
        {screen==='resources'&&<>
          <div className="eyebrow">02 / RESOURCES</div><h1 className="compact">给它一点空间。</h1><p className="intro">资源只供这套虚拟机使用。<br/>关闭虚拟机后，内存与处理器将释放给你的电脑。</p>
          <StorageLocation host={host} busy={navLocked} onChoose={chooseStorage}/>
          <ResourceGrid host={host} resources={resources} onChange={setResources} disabled={navLocked}/>
          {!diskFits&&<p className="error-text">所选位置可用 {host.hardware.free_disk_gib} GB，无法分配当前磁盘容量。请调整容量或更换存放位置。本平台磁盘最小值为 {host.runtime_profile.min_disk_gib} GB，创建前需至少 {product.limits.min_free_disk_gib} GB 可用空间。</p>}
          <div className="summary"><div className="section-line"><span>即将创建</span><span>LOCAL ONLY</span></div><dl><div><dt>操作系统</dt><dd>Omarchy <span>{host.assets.image_version}</span></dd></div><div><dt>默认 Agent</dt><dd>Claude Code</dd></div><div><dt>网络节点</dt><dd>{subscription?.nodes.find(n=>n.index===selected)?.name}</dd></div><div><dt>地区与语言</dt><dd>随所选节点的实测出口设置</dd></div><div><dt>存储位置</dt><dd className="path">{host.data_directory}/vm</dd></div></dl></div>
          <div className="quiet-note"><Monitor size={16}/><p>请在 Omarchy 窗口中完成首次账号设置。应用语言时会正常关闭客机 Chromium，随后按提示重启环境并核验结果。</p></div>
          {!host.assets.image_ready&&<p className="field-note">所选位置尚无系统镜像。可以下载到这里，也可以使用已准备好的离线资源包。</p>}
          {!host.assets.image_ready&&<AssetRow host={host} busy={navLocked} onChooseAssets={chooseAssets}/>}
          {view.task==='download'&&<DownloadProgress view={view}/>}
          <footer><button className="back" disabled={navLocked} onClick={()=>setStep(1)}><ChevronLeft size={16}/>上一步</button>{host.assets.image_ready
            ?<button className="primary" disabled={navLocked||!diskFits} onClick={apply}>创建独立环境 <ArrowRight size={17}/></button>
            :<button className="primary" disabled={navLocked||!host.assets.ready} onClick={download}>{view.task==='download'&&<LoaderCircle className="spin" size={16}/>}下载 Omarchy</button>}</footer>
        </>}
        {screen==='progress'&&<Progress view={view} busy={busy} onStop={stop}/>}
        {screen==='network'&&<>
          <div className="eyebrow">NETWORK</div><h1 className="compact">{view.environment==='incomplete'?'选一个节点，继续配置。':'更换网络节点。'}</h1><p className="intro">粘贴订阅链接或节点链接。账号和工作文件都会保留。<br/>应用后按新节点的实测出口更新语言和时区，再重启一次完成核验。</p>
          <NetworkPicker {...picker}/>
          {view.running&&<div className="quiet-note"><Power size={16}/><p>环境正在运行，应用节点时会先正常关机。</p></div>}
          <footer><button className="back" onClick={()=>setPage('home')}><ChevronLeft size={16}/>返回我的环境</button><button className="primary" disabled={selected===null||busy} onClick={apply}>{busy?<LoaderCircle className="spin" size={16}/>:null}{view.running?'关机并应用':'应用此节点'} <ArrowRight size={17}/></button></footer>
        </>}
        {screen==='shortcuts'&&<Shortcuts platform={host.platform}/>}
        {screen==='home'&&<Home host={host} view={view} busy={busy} hostAccessPending={hostAccessPending} chatReady={!!chatUser} onChat={()=>setPage('chat')} onDesktop={showDesktop} onStart={start} onStop={stop} onRestart={restart} onConfigure={()=>setPage('network')} onChooseAssets={chooseAssets} onHostAccess={changeHostAccess} onSoftwareRendering={changeSoftwareRendering}/>}
      </div>}
    </main>
  </div>;
}
function Welcome({host,view,busy,onChooseAssets,onChooseStorage,onDownload,onNext}:{host:Host;view:View;busy:boolean;onChooseAssets:()=>void;onChooseStorage:()=>void;onDownload:()=>void;onNext:()=>void}) {
  const downloading = view.task === 'download';
  const downloadBlocked = !host.supported||!host.assets.ready||busy||view.task!==null;
  const configurationBlocked = downloadBlocked||!host.hardware.virtualization_ready;
  return <>
    <div className="eyebrow">A SPACE OF YOUR OWN</div>
    <h1>把工作，留在<br/><em>独立的空间里。</em></h1>
    <p className="intro">在你的电脑里，准备一套全新的 Omarchy。<br/>配好网络，打开 Claude Code，就可以开始。</p>
    <div className="environment-diagram">
      <div className="diagram-host"><Monitor size={17}/><span>你的电脑</span><span className="small-label">macOS / Windows</span></div>
      <div className="diagram-guest"><div className="guest-caption"><span className="status-dot"/> 独立虚拟环境 <span className="small-label">OMARCHY</span></div><div className="guest-parts"><div><Terminal size={22}/><b>Claude Code</b><span>默认 Agent</span></div><div className="diagram-plus">+</div><div><Network size={22}/><b>sing-box</b><span>客机网络服务</span></div><div className="diagram-plus">+</div><div><HardDrive size={22}/><b>独立磁盘</b><span>项目与账号留在这里</span></div></div></div>
    </div>
    <div className="principles"><span><LockKeyhole size={15}/>本机文字可粘贴进虚拟机，虚拟机默认访问不到本机</span><span><KeyRound size={15}/>独立登录 Claude 账号</span></div>
    <AssetRow host={host} busy={busy} onChooseAssets={onChooseAssets}/>
    <StorageLocation host={host} busy={busy} onChoose={onChooseStorage}/>
    <div className="hardware-line"><span>{host.cpu_count} 核处理器 · {host.hardware.memory_mib/1024} GB 内存 · {host.hardware.free_disk_gib} GB 可用空间</span><span>{host.hardware.prerequisite}</span></div>
    {host.supported&&!host.hardware.virtualization_ready&&<p className="field-note">可以先下载系统镜像；启用硬件虚拟化后再创建环境。</p>}
    {!host.supported&&<p className="error-text">当前版本支持 Apple Silicon Mac 和 Windows x86_64。</p>}
    {downloading&&<DownloadProgress view={view}/>}
    <footer><p>首次使用要从 Omarchy 官方下载系统镜像。安装 Claude Code 和使用 Claude 也都需要联网。</p>{host.assets.image_ready
      ?<button className="primary" disabled={configurationBlocked} onClick={onNext}>开始配置 <ArrowRight size={17}/></button>
      :<button className="primary" disabled={downloadBlocked} onClick={onDownload}>{downloading&&<LoaderCircle className="spin" size={16}/>}下载 Omarchy（约 {((host.assets.download_bytes??0)/1024**3).toFixed(1)} GB）</button>}</footer>
  </>;
}
function StorageLocation({host,busy,onChoose}:{host:Host;busy:boolean;onChoose:()=>void}) {
  return <section className="storage-location" aria-label="存放位置"><div className="storage-heading"><HardDrive size={18}/><strong>存放位置</strong><span>可用 {host.hardware.free_disk_gib} GB</span><button className="text-button" disabled={busy||!host.storage.can_change} onClick={onChoose}>选择存放位置 <FolderOpen size={15}/></button></div><p className="storage-path">{host.data_directory}</p><p className="field-note">虚拟磁盘和在线下载的系统镜像都保存在这里，重新打开软件仍会使用此位置。{host.storage.can_change?' 更换位置不会搬动原位置已有的下载或未完成文件。':' 已有环境暂不支持更换位置。'}</p></section>;
}
function DownloadProgress({view}:{view:View}) {
  return <div className="download-progress"><div><i style={{width:`${(view.progress??0)*100}%`}}/></div><span>{view.message}。关闭软件后再打开，会接着下载。</span></div>;
}
function AssetRow({host,busy,onChooseAssets}:{host:Host;busy:boolean;onChooseAssets:()=>void}) {
  return <div className={`asset-row ${host.assets.ready?'available':''}`}><Disc3 size={18}/><div><strong>{host.assets.message}</strong><span>{host.assets.ready?`Omarchy ${host.assets.image_version}${host.assets.image_ready?' 已下载':'，首次使用需要下载'}`:'运行组件包含虚拟机运行时和 sing-box'}</span></div><button className="text-button" disabled={busy} onClick={onChooseAssets}>选择资源包 <FolderOpen size={15}/></button></div>;
}
function NetworkPicker({input,setInput,subscription,selected,setSelected,busy,onImport}:{input:string;setInput:(value:string)=>void;subscription:Import|null;selected:number|null;setSelected:(index:number)=>void;busy:boolean;onImport:()=>void}) {
  return <>
    <div className="input-panel"><label htmlFor="subscription">订阅 / 节点链接</label><div className="input-combined"><KeyRound size={18}/><input id="subscription" type="password" autoComplete="off" spellCheck={false} value={input} placeholder="https://… 或 vless://、trojan://、ss://…" onChange={e=>setInput(e.target.value)}/><button className="secondary" disabled={!input.trim()||busy} onClick={onImport}>{busy?<LoaderCircle className="spin" size={16}/>:<ArrowUpRight size={16}/>}读取配置</button></div><p className="field-note">支持常用节点链接、Base64 订阅、Clash YAML 与 sing-box JSON。暂不支持的节点会单独说明。</p></div>
    {subscription?<div className="node-list"><div className="section-line"><span>选择使用的节点</span><span>{subscription.nodes.length} 个可用节点</span></div><div className="node-scroll">{subscription.nodes.map(n=><button className={`node ${n.index===selected?'selected':''}`} key={n.index} onClick={()=>setSelected(n.index)}><span className="radio">{n.index===selected&&<i/>}</span><Globe2 size={19}/><strong>{n.name}</strong><span className="protocol">{n.protocol}</span>{n.index===selected&&<Check size={16}/>}</button>)}</div>{subscription.notices.length>0&&<details><summary>{subscription.notices.length} 条格式说明</summary>{subscription.notices.map((n,i)=><p key={i}>{n}</p>)}</details>}</div>:<div className="empty-network"><div className="network-orbit"><Globe2 size={28}/></div><p>连接信息只用于这套虚拟环境。</p><span>读取后，你可以在这里选择具体节点。</span></div>}
  </>;
}
function ResourceGrid({host,resources,onChange,disabled=false}:{host:Host;resources:Resources;onChange:(resources:Resources)=>void;disabled?:boolean}) {
  const memoryMax = Math.min(product.limits.max_memory_mib, host.hardware.memory_mib - product.limits.host_memory_mib)/1024;
  return <div className="resource-grid">
    <Resource icon={<Cpu/>} label="处理器" unit="核" value={resources.cpus} min={product.limits.min_cpus} max={host.cpu_count} step={1} disabled={disabled} onChange={cpus=>onChange({...resources,cpus})}/>
    <Resource icon={<Monitor/>} label="运行内存" unit="GB" value={resources.memory_mib/1024} min={product.limits.min_memory_mib/1024} max={memoryMax} step={1} disabled={disabled} onChange={gb=>onChange({...resources,memory_mib:gb*1024})}/>
    <Resource icon={<HardDrive/>} label="磁盘容量" unit="GB" value={resources.disk_gib} min={host.runtime_profile.min_disk_gib} max={Math.max(host.runtime_profile.min_disk_gib,host.storage.disk_limit_gib)} step={1} disabled={disabled||host.storage.disk_limit_gib<host.runtime_profile.min_disk_gib} onChange={disk_gib=>onChange({...resources,disk_gib})}/>
  </div>;
}
function Resource({icon,label,unit,value,min,max,step,disabled=false,onChange}:{icon:React.ReactNode;label:string;unit:string;value:number;min:number;max:number;step:number;disabled?:boolean;onChange:(value:number)=>void}) {
  return <div className="resource"><div className="resource-label">{icon}<span>{label}</span></div><div className="resource-value"><input type="number" aria-label={label} value={value} min={min} max={max} step={step} disabled={disabled} onChange={e=>onChange(Number(e.target.value))}/><span>{unit}</span></div><input aria-label={`${label}滑块`} type="range" min={min} max={max} step={step} value={value} disabled={disabled} onChange={e=>onChange(Number(e.target.value))}/><div className="range-label"><span>{min} {unit}</span><span>{max} {unit}</span></div></div>;
}
function Progress({view,busy,onStop}:{view:View;busy:boolean;onStop:()=>void}) {
  const install = view.task === 'install';
  const shown = install ? stages : stages.slice(1);
  const index = shown.findIndex(s=>s.id===view.phase);
  return <>
    <div className="eyebrow">{install?'03 / BUILDING YOUR SPACE':'NETWORK'}</div><h1 className="compact">{install?'正在准备你的环境。':'正在配置你的环境。'}</h1><p className="intro">{view.message}</p>
    <div className="deployment-layout"><div className="deployment-steps">{shown.map((s,i)=><div className={`deployment-step ${i<index?'done':''} ${i===index?'current':''}`} key={s.id}><span className="deployment-marker">{i<index?<Check size={16}/>:i===index?<LoaderCircle size={17} className="spin"/>:i+1}</span><div><strong>{s.label}</strong><p>{s.detail}</p></div></div>)}</div><div className="install-card"><div className="mini-window"><span/><span/><span/><Terminal size={45}/></div><b>一个全新的开始</b><p>系统与工具都安装在<br/>虚拟机的独立磁盘里。</p><span className="install-tag">OMARCHY × CLAUDE CODE</span></div></div>
    <footer><p>数据会保留。中止后回到我的环境，可以继续配置。</p>{view.running&&<button className="secondary" disabled={busy} onClick={onStop}><Power size={16}/>中止并关机</button>}</footer>
  </>;
}
function combo(platform:string,keys:string[]) {
  const names = (product.shortcuts.keys as Record<string,Record<string,string>>)[platform];
  return keys.map(key=>names[key]??key);
}
function Shortcuts({platform}:{platform:string}) {
  return <>
    <div className="eyebrow">DAILY SHORTCUTS</div><h1 className="compact">常用快捷键。</h1><p className="intro">{(product.shortcuts.note as Record<string,string>)[platform]}</p>
    <div className="shortcut-grid">{product.shortcuts.groups.map(group=><section key={group.title}><h2>{group.title}</h2>{group.items.map(item=><div className="shortcut" key={item.action}><span>{item.action}</span><span className="keys">{combo(platform,item.keys).map(key=><kbd key={key}>{key}</kbd>)}</span></div>)}</section>)}</div>
  </>;
}
function Home({host,view,busy,hostAccessPending,chatReady,onChat,onDesktop,onStart,onStop,onRestart,onConfigure,onChooseAssets,onHostAccess,onSoftwareRendering}:{host:Host;view:View;busy:boolean;hostAccessPending:boolean;chatReady:boolean;onChat:()=>void;onDesktop:()=>void;onStart:()=>void;onStop:()=>void;onRestart:()=>void;onConfigure:()=>void;onChooseAssets:()=>void;onHostAccess:(enabled:boolean)=>void;onSoftwareRendering:(enabled:boolean)=>void}) {
  const {environment,running,delivery} = view;
  const starting = view.task === 'start', stopping = view.phase === 'stopping', failed = view.phase === 'failed';
  const nodeFailed = failed && view.failure?.kind === 'node';
  const waiting = starting || stopping, locked = busy || waiting;
  const incomplete = environment === 'incomplete', needsRestart = environment === 'restart_required';
  const [title,subtitle,intro] = stopping ? ['正在关机。','数据会保留。',view.message]
    : starting ? ['正在启动。','完成后自动核验。',view.message]
    : incomplete ? ['配置还没完成。','选好节点继续。','账号与工作文件都保留在虚拟机里。选择一个节点后继续配置。']
    : nodeFailed ? ['网络节点需要处理。','环境和文件都还在。','虚拟机只走订阅链接里的节点。更新链接或换一个节点后，环境就能继续。']
    : needsRestart ? ['地区已配置。','重启后开始使用。',running?'系统和浏览器已按实测出口设置语言与地区。请重启环境，完成生效核验。':'启动环境，完成语言与地区的生效核验。']
    : failed ? ['这次没有成功。','环境和文件都还在。',running?'请按上方错误提示处理，可先正常关机后再启动。':'请按上方错误提示处理，然后重新启动环境。']
    : running ? ['空间就绪。','开始你的工作。','Claude Code 已在虚拟机里运行。在对话里交代任务，需要时切到终端或打开桌面。']
    : ['空间已保存。','随时可以继续。','你的环境与文件已保存在本地。启动后即可继续。'];
  const next = waiting ? null : needsRestart ? ['生效步骤','重启 Omarchy 应用语言设置','重启后检查语言、固定时区和浏览器配置，通过后才会显示就绪。']
    : chatReady ? ['下一步','进入对话，使用 Claude Code','首次使用先在终端里登录 Claude。账号与工作文件保留在虚拟机里。'] : null;
  const region = delivery?.region;
  return <>
    <div className="eyebrow">YOUR LOCAL WORKSPACE</div>
    <div className="ready-heading"><h1 className="compact">{title}<br/><em>{subtitle}</em></h1>{environment==='ready'&&running&&!waiting&&!failed&&<div className="ready-seal"><Check size={30}/></div>}</div>
    <p className="intro">{waiting&&<LoaderCircle className="spin" size={14}/>}{intro}</p>
    {next&&(chatReady
      ?<button className="next-action" onClick={onChat}><span>{next[0]}</span><div><strong>{next[1]}</strong><p>{next[2]}</p></div><ArrowUpRight size={23}/></button>
      :<div className="next-action"><span>{next[0]}</span><div><strong>{next[1]}</strong><p>{next[2]}</p></div><ArrowUpRight size={23}/></div>)}
    {!host.assets.ready&&<AssetRow host={host} busy={busy} onChooseAssets={onChooseAssets}/>}
    <div className="delivery-card">
      <div className="delivery-top"><div className="brand-mark small"><i/><i/><i/></div><div><b>Omarchy 工作环境</b><span>{host.assets.image_version}</span></div><span className={`runtime-state ${running?'online':''}`}><span className="status-dot"/>{stopping?'正在关机':starting?'正在启动':running?'正在运行':'已关闭'}</span></div>
      {delivery&&<><div className="delivery-items"><div><Terminal size={20}/><span>默认 Agent</span><strong>Claude Code</strong></div><div><Network size={20}/><span>网络服务</span><strong>sing-box</strong></div><div><HardDrive size={20}/><span>数据存储</span><strong>独立本地磁盘</strong></div></div><div className="delivery-detail">{delivery.claude_version} · {delivery.sing_box_version} · 出口 {delivery.egress_country}</div></>}
    </div>
    {region&&<div className="region-result"><Globe2 size={17}/><span>实测出口 {region.country} · 系统 {region.locale} · 浏览器 {region.browser_language}<br/>固定时区 {region.timezone}</span></div>}
    {delivery?.proxy_required&&delivery.network_guard_active&&<div className="quiet-note"><LockKeyhole size={16}/><p>网络保护已启用：应用只能通过代理联网，代理停止时自动断网。</p></div>}
    {host.graphics.available&&<div className="host-access"><Monitor size={18}/><div><strong>兼容显示</strong><p>{host.graphics.software_rendering?'已开启，使用 CPU 绘制桌面，图形性能会有所降低。此选择会保留到下次启动。':'图形加速无法启动时可开启，继续使用原来的环境和文件。'}{running?' 请先正常关机再更改。':''}</p></div><button className="secondary" disabled={locked||running} onClick={()=>onSoftwareRendering(!host.graphics.software_rendering)}>{host.graphics.software_rendering?'关闭兼容显示':'开启兼容显示'}</button></div>}
    {environment==='ready'&&<div className="host-access"><Terminal size={18}/><div><strong>允许虚拟机访问本机</strong><p>{hostAccessPending&&host.platform==='windows-x86_64'?'正在准备本机 SSH 服务。请在 Windows 管理员授权窗口中确认；首次安装可能需要几分钟。':!view.host_access?`打开后，虚拟机里的 Claude Code 可以用 ssh ${product.host_access.alias} 操作这台电脑，权限和你一样。`:delivery?.host_access_active?`已打开。在虚拟机里用 ssh ${product.host_access.alias} 操作这台电脑。`:'已打开，重启虚拟机后生效。'}</p></div><button className="secondary" disabled={locked} onClick={()=>onHostAccess(!view.host_access)}>{hostAccessPending?<><LoaderCircle className="spin" size={16}/>正在打开</>:view.host_access?'关闭':'打开'}</button></div>}
    <footer><p>数据保留在本地独立磁盘。</p><div className="actions">
      {running&&<button className="secondary" disabled={busy||stopping} onClick={onDesktop}><Monitor size={17}/>打开桌面</button>}
      {running&&<button className="secondary" disabled={busy||stopping} onClick={onStop}><Power size={17}/>正常关机</button>}
      {chatReady&&<button className="primary" onClick={onChat}><MessageSquare size={16}/>进入对话</button>}
      {incomplete||nodeFailed?<button className="primary" disabled={locked} onClick={onConfigure}>{incomplete?'继续配置':'更换网络节点'} <ArrowRight size={17}/></button>
        :needsRestart&&running?<button className="primary" disabled={locked} onClick={onRestart}><RefreshCw size={17}/>重启并完成设置</button>
        :!running&&<button className="primary" disabled={locked} onClick={onStart}>{needsRestart?'启动并完成核验':'启动环境'} <Play size={16}/></button>}
    </div></footer>
  </>;
}
createRoot(document.getElementById('root')!).render(<App/>);
