# 架构与客机协议

## 边界

桌面 UI 只表达流程、用户选择和实际状态。Rust 负责硬件查询、订阅下载转换、磁盘创建、QEMU 子进程与状态保存。平台参数在 `runtime-profiles.json`，不依赖 Try Omarchy 的全局偏好。

客机配置由 `guest/provision.py`、`guest/network.py` 和 `guest/region.py` 执行。桌面端通过 SMBIOS 的公开 systemd 凭据提供一个临时服务和 `multi-user.target` 的依赖。程序和公开参数以 zlib 压缩后传入，避免地区映射扩大 SMBIOS 内容；订阅凭据不进入命令行或 SMBIOS。

```mermaid
flowchart LR
    U[输入订阅或节点] --> R[Rust 解析 / 内存中选择节点]
    A[官方镜像与内置运行时] --> Q[QEMU 独立磁盘]
    R --> S[专用 virtio 串口]
    S --> G[客机配置程序]
    G --> N[sing-box systemd 服务]
    N --> L[实测出口 IP → 系统/浏览器语言、地区与固定时区]
    L --> C[Claude Code 与默认 Agent]
    C --> B[重启后的地区核验]
    B --> V[服务和出口核验]
    V --> D[真实交付回执]
```

## 镜像下载源

Windows v0.13.0 镜像从 DMIT `download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/` 下载，Mac 沿用官方 GitHub。源参数在 `runtime-profiles.json`；桌面运行时读取资源目录的 `manifest.json`，所以已有 Windows 客户端更换清单即可生效，无需重新编译 EXE。

DMIT 使用既有 Nginx 静态下载站，镜像在 Web 根目录外下载，按官方发行附件的长度与 SHA-256 验证后，整体重命名到不可变的版本目录。客户端下载保持原有 HTTPS Range 断点续传，不变更文件名或缓存位置。本轮只在发布端验证 SHA-256；当前客户端下载器仍只检查下载字节数，未新增客户端哈希或自动源切换。运维与回滚见 [image-mirror.md](image-mirror.md)。

## 桌面命令

| Tauri 命令 | 输入 | 结果 |
| --- | --- | --- |
| `inspect_host` | 无 | 平台、CPU/内存/空间、虚拟化能力、资源包和当前 `View` |
| `select_asset_pack` | 资源包目录 | 空闲时检查并在本次运行中选择资源包 |
| `select_storage_directory` | 数据根目录 | 空闲且尚无环境时保存位置，供 vm 和在线镜像缓存共同使用；不搬迁旧数据 |
| `download_image` | 无 | 下载清单 `downloads` 列出的系统镜像，进度由事件报告 |
| `import_subscription` | 链接或订阅文本 | 节点名称/协议/索引和未支持项；敏感 outbound 留在 Rust 内存 |
| `start_install` | 节点索引、资源规格 | 发起创建或继续配置，后续由事件报告 |
| `start_vm` | 无 | 启动已配置或等待重启核验的环境，重新检查客机服务；出口 IP 未变时沿用已测地区 |
| `stop_vm` | 无 | 发送 QMP `system_powerdown`，等待正常退出 |
| `current_state` | 无 | 当前阶段、消息、运行状态和不含凭据的回执 |
| `chat_open` | 事件代次、指定会话（可空）、是否新对话、终端列数行数 | 经 SSH 准备启动设置，在虚拟机终端里启动或恢复 Claude Code，并开始跟随会话记录、钩子和状态栏 |
| `chat_sessions` | 无 | 工作目录下的历史会话（编号、标题、最近修改时间），新的在前 |
| `chat_open_link` | 网址 | 在虚拟机浏览器里打开对话中的网页链接 |
| `chat_send` | 文字 | 以粘贴方式写入终端，随后单独回车 |
| `terminal_input` / `terminal_resize` | 按键数据 / 列数行数 | 终端视图的原始输入与窗口大小 |
| `chat_close` | 无 | 断开对话的两条 SSH 连接 |
| `show_desktop` | 无 | 显示并置前虚拟机桌面窗口 |
| `browser_open` | 事件代次、尺寸（宽高与缩放） | 在虚拟机里把浏览器放上独立虚拟屏幕并启动画面服务，返回本机 WebSocket 地址 |
| `browser_resize` / `browser_close` | 尺寸 / 无 | 调整虚拟屏幕尺寸；断开浏览器连接 |

事件名为 `deployment`。创建阶段依次是 `image`、`boot`、`network_install`、`account`、`region`、`claude_install`、`verify`、`restart_required`；重启核验通过后才进入 `ready`。错误为 `failed`，生命周期还包括 `stopping`、`stopped`。实际步骤由客机事件推动，没有计时伪进度。

## 生命周期与界面

`View` 是界面唯一读取的生命周期记录，由 Rust 维护并通过 `deployment` 事件推送：

| 字段 | 取值 | 含义 |
| --- | --- | --- |
| `environment` | `none`、`incomplete`、`restart_required`、`ready` | 由 `machine.json` 与 `delivery.json` 推导的环境状态，只在 `vm::lifecycle` 计算 |
| `task` | `install`、`reconfigure`、`start`、`download`、`null` | 正在执行的任务；`vm::finish` 是所有任务的唯一结束出口 |
| `running` | 布尔 | QEMU 进程是否存在 |
| `phase`、`message` | 见上节 | 当前阶段与说明 |
| `delivery` | 回执或 `null` | 上一次成功配置的客机回执 |
| `failure` | `{kind, detail}` 或 `null` | 失败分类；`kind=node` 表示要到“网络节点”处理，`detail` 是技术原因，只写入 `deployment.json` |

页面按固定优先级推导，前端不保存“走到了哪一步”：`install`/`reconfigure` 进行中显示进度页 → 用户打开了“网络节点”显示节点页 → `environment=none` 显示创建向导 → 其余显示“我的环境”。向导的步骤号只在 `none` 时有意义，所以环境存在后不可能再落回向导。

节点只有一个来源：软件里的订阅或节点链接。开始重新配置时清除旧回执并置 `configured=false`，因为客机内的节点凭据会被替换；中途失败或中止后状态是 `incomplete`，不回退到旧节点，由“继续配置”应用可用节点。已读取的节点保留在 Rust 内存，直到配置成功，失败后可以直接换一个节点重试。

## 强制代理与启动顺序

Rust 以 `-S` 暂停 CPU 启动 QEMU，先断开虚拟网卡再继续启动，阻止出厂镜像和旧版环境在保护安装前联网。客机持久 `omarchy-space-guard` 在 `network-pre.target` 前加载；停止服务不会清空规则。规则更新使用 nft 原子事务，检查失败时保留原规则并保持链路断开。

保护规则先检查应用目的地址，再在 postrouting 检查最终出口；只允许回环、TUN、独立代理系统 UID 和 DHCP/NDP，不对 established 连接做通用放行。私网/宿主目的地址在应用层规则和 SB 路由同时拒绝。纯 TUN 使用 gvisor，避免出厂 UFW 对 mixed 栈回流新连接的阻断；UFW 保持启用。

SB 以 `omarchy-space-net` 运行，依赖断网保护，在 multi-user 完成前启动，`Restart=always`、重启间隔 2 秒。NetworkManager 的 up/DHCP 事件启动 SB；配置过程和重连 hook 共享 flock，避免同时操作服务。每次 SB 启动重新为 TUN 注册 systemd-resolved DNS。参数均来自根配置 `network`。

客机只走 IPv4，`network.dns_strategy` 由 `apply_network_policy` 在每次开机写入 DNS 和路由，同时拒绝 IPv6 目的地址。gvisor TUN 会在本地完成 TCP 握手，节点没有 IPv6 出口时，优先走 IPv6 的程序看到“已连接”后一直等到超时。

服务 active、TUN 存在和代理端口监听只是就绪前提。出口核验仍要求显式代理和 TUN 的真实 HTTPS 请求成功且 IP 相同。启动期间的临时连接重置最多重试 3 次，每次间隔 1 秒；最终回执记录各路径尝试次数。持续失败、错误响应或出口不一致均拒绝就绪，不改走直连。之后再用 `curl` 访问一次：程序选地址的方式和 urllib 不同，只测 urllib 会漏掉上述 IPv6 问题。

## 出口关联环境

先确认 TUN 和显式代理请求使用相同出口 IP，再通过所选代理查询该 IP 的国家/地区、行政区及 IANA 时区。国家/行政区匹配根配置中的明确语言映射；时区直接采用定位结果，不能由国家或节点名称猜测。当前定位接口是 [ipapi](https://ipapi.co/api/)，接口和字段名均可在根配置调整。

应用范围为系统 `LANG`、`LANGUAGE`、各项 `LC_*` 地区格式、用户会话 locale、固定时区、Chromium 启动语言和各现有 profile 的 `intl.selected_languages` 首选语言。`intl.accept_languages` 由 Chromium 派生，可能在写盘时省略，不作为持久配置校验条件。只保存被改动的浏览器语言字段用于回滚，不复制浏览器账号或凭据。修改前正常退出客机 Chromium。

Mac ARM 客机使用 `try-omarchy-timezone --select` 并核实模式为 `fixed`；Windows x86_64 客机使用 `timedatectl` 设置固定时区，QEMU 不提供宿主时区桥接。系统语言在新会话生效，因此记录应用时的客机 boot ID；同一次启动不能返回完成。重启后复测出口、时区、locale、浏览器配置、Claude 和服务，通过后才置 `configured=true`。

安装和重新选择网络时应用地区。普通启动若发现出口对应的国家、时区或语言配置变化，会提示重新配置网络；同一语言/时区内的行政区变化只更新定位回执。不会自动切回美国或宿主设置。

## 串口格式

端口名由 `product.config.json` 指定。QEMU 通过仅监听宿主回环地址的临时 TCP 端口接到 Rust，客机看到 `/dev/virtio-ports/<name>`。

1. 客机发送一行 JSON：`phase=connected`，含是否有安装记录。
2. 桌面发送一行 JSON：`action=configure` 或 `action=status`。
3. 配置操作携带 `network`（完整 sing-box JSON）、`sing_box_bytes`。
4. JSON 换行后，桌面发送相应长度的 sing-box 包原始字节。客机流式接收到 `/run`，不使用宿主共享目录。
5. 客机安装独立防火墙后发送 `network_guard_ready` 和策略版本。Rust 校验版本并通过 QMP 接通网卡，回复 `{"network_up":true}`；客机等待地址后启动 SB。首次安装和每次启动都执行此握手。
6. 客机逐行发送阶段 JSON。成功回执包括工具版本、默认 Agent、TUN 与显式代理出口是否一致、出口国家；不回传节点密码、UUID、订阅 URL 或账号凭据。`ready` 必须包含 `proxy_required=true`、`network_guard_active=true`，且当次网络握手已成功。

## 访问本机

“允许虚拟机访问本机”默认关闭。打开后，下一次开机时客机为账号生成 SSH 密钥，私钥留在客机，公钥经回执交给桌面端，校验格式后写入宿主 `~/.ssh/authorized_keys`，限定来源为回环地址并禁用转发。QEMU 用户网络把 `host_access.address` 映射到宿主回环。

断网保护只放行账号 UID 到该地址 SSH 端口的 TCP，TUN 用 `route_exclude_address` 排除该地址，其他内网和宿主端口继续拦截。客机写入 `~/.ssh/config` 的 `host` 别名和 `~/.claude/CLAUDE.md` 的说明。关闭时桌面端立即删除授权行，下一次开机撤销规则和别名。

Windows 的 set_host_access 在后台任务中准备服务，操作锁避免与安装或位置切换交叉。hostaccess_windows.rs 负责检查已管理配置、UTF-16 编码参数、ShellExecuteExW runas 和提权结果；scripts/windows-host-ssh.ps1 负责 capability 安装、配置备份与改写、启动/自动启动/重启，结果存为 host-ssh-setup/随机ID.json。用户目录和公钥仍由未提权的原桌面进程管理。未就绪或仍有管理员密钥覆盖时只发起一次 UAC；已经配置且监听正常时直接继续。

新建 ProgramData/ssh 后立即应用与 sshd 相同的受保护 DACL（SYSTEM/Administrators 完全控制，Authenticated Users 读取/执行），成功后才继续配置；已存在的目录不改 ACL。

监听与全局 AuthorizedKeysFile 在第一次启动前写好，默认 administrators 匹配块被注释；有其他自定义指令时保留其作用域，仅去掉密钥路径覆盖。Include 外部配置不自动改写。平台错误提示位于 runtime-profiles.json 的 host_access_service_hint，Mac 只读检查不安装。开关失败不写入 machine.host_access，成功后客机仍在下次启动应用网络规则和别名。

命令在宿主执行，走宿主网络，不经过所选节点。

## 对话与终端

软件名为 GoToCS（Go to Claude Space）。应用标识仍是 `dev.omarchy.space`，数据目录和已有环境原样沿用。参数在 `product.config.json.terminal`，宿主 ssh / ssh-keygen 路径在 `runtime-profiles.json` 各平台的 `terminal`。

对话是虚拟机终端里 Claude Code 的一个视图，做法参照 Orca 的终端托底对话：

- 通道：每次启动在 QMP 建立后用 `human-monitor-command` 执行 `hostfwd_add`，把 `terminal.forward_address` 的一个空闲端口转到客机 `terminal.guest_port`。转发在运行时添加，旧离线包清单不需要新字段。
- 密钥：宿主用平台自带 ssh-keygen 在默认数据目录 `terminal/` 生成 ed25519 密钥（用户目录的权限满足 Windows OpenSSH 对私钥的要求），公钥随配置/状态载荷交给客机，写入账号 `~/.ssh/authorized_keys` 的受管块。known_hosts 在 `vm/` 下，随环境一起存在。
- 客机：每次开机配置程序启用 sshd，UFW 放行宿主地址到该端口。断网保护只放行这条连接的回包（`ct direction reply`）去往宿主地址；TUN 始终用 `route_exclude_address` 排除宿主地址，回包不进 TUN。保护规则版本为 2。
- 会话：先开一条 ssh 把 `guest/transcript_follow.py` 经 stdin 交给客机 python3。它在 `terminal.runtime_directory` 写好 Claude 的启动设置（`terminal.claude_settings`，加上把各钩子输入追加到文件的钩子、把状态栏输入存成文件的状态栏命令；账号自己配置的状态栏仍按原输入运行），回报 `gotocs_ready` 后，宿主伪终端（portable-pty）才运行 `ssh -tt`，在账号的登录 shell 里进入 `~/<working_directory>`，以 `terminal.claude_arguments`（完全访问）和 `--settings <该文件>` 启动 Claude。会话编号存于 `vm/chat.json`：已有记录用 `--resume`，否则 `--session-id`；从历史里选中的会话直接指定编号。ssh 会话带上桌面的 `WAYLAND_DISPLAY`，Claude 登录时在虚拟机浏览器打开登录页并收到本地回调。
- 对话内容：跟随脚本读 `~/.claude/projects/<目录>/<会话>.jsonl`，只回传界面要显示的记录种类（`terminal.record_types`，附件里只要中途取走的排队消息），去掉图片数据和结构化工具结果，其余请求记录留在虚拟机。标记行：`gotocs_session`（跟随的会话变了）、`gotocs_live`（历史与实时的分界）、`gotocs_status`（登录、首次引导、工作目录是否已信任）、`gotocs_hook`（钩子）、`gotocs_meter`（状态栏数字）。`/clear`、`/resume` 换会话由 SessionStart 钩子给出新会话和记录路径。Rust 的 `transcript.rs` 把记录压缩为对话条目（用户消息、回答、思考、工具调用与结果、回合结束及用时、中断、命令及其输出、压缩、排队、标题），钩子按到达顺序夹在条目之间，经事件 `chat` 推给界面；终端输出走 `terminal`，终端结束发 `chat-exit`。
- 输入：对话框发送是一次 bracketed paste 写入正文，再隔 `submit_delay_milliseconds` 单独写回车（与正文同一次写入的回车会被当成粘贴内容）；停止是 Esc；终端视图直接写原始按键。终端输出在 `output_gather_milliseconds` 内到达的多次写入合成一个事件，界面重绘一次。
- 状态：由钩子按到达顺序推出。UserPromptSubmit、PostToolUse、PostToolUseFailure 为“正在工作”；PermissionRequest 和权限类 Notification 为“等你回答”；Stop、SessionStart、空闲 Notification 以及记录里的中断为“空闲”。同一轮询里先读会话记录再读钩子，上一轮的中断因此排在下一轮的提交之前。Stop 钩子通常比这一轮最后一段回答早到约 0.2 秒，界面按回合收尾处理。未登录、未完成首次引导或工作目录尚未信任时界面切到终端并给出提示。
- 界面：一问一答为一个回合，进行中按连续的工具调用分组折叠，结束后收成“用时—步数”，只留问题与最终回答，可逐级展开到每次调用的输入输出。Claude 提出单个单选问题时在对话里给选项，点选即向终端发送对应数字键；其余等待情形提示到终端回答。上下文占用、模型、思考强度和订阅额度取自状态栏。历史对话由 `guest/session_list.py` 列出（标题取 Claude 给会话起的名字，没有时取第一句话）。对话里的网页链接经 `terminal.link_command` 在虚拟机浏览器打开，不在本机打开。界面参数在 `product.config.json.chat`。
- 桌面：普通启动时 QEMU 窗口在启动任务结束前保持隐藏（Mac 用 NSRunningApplication，Windows 用 ShowWindow），“打开桌面”显示并置前。创建和重新配置不隐藏，首次账号设置要在窗口里完成。

## 虚拟机浏览器

浏览器始终在虚拟机里运行，出口、时区、语言和浏览器特征都随所选节点；软件的“浏览器”页只显示它的画面。参数在 `product.config.json.browser`。

- 组件：每次开机配置程序在网络核验后执行 `pacman -S --needed` 安装 wayvnc（Omarchy ARM 镜像 2026-11 实测可下），已装时不联网。
- 虚拟屏幕：`browser_open` 经 SSH 以账号身份运行 `guest/browser_stream.py`（作为 `python3 -c` 参数传入，stdin 留作尺寸通道）。客机 Hyprland 0.56 用 Lua 配置，`hyprctl keyword` 和旧式 dispatch 都不可用：分辨率和工作区规则用 `hyprctl eval 'hl.monitor{...}' / 'hl.workspace_rule{...}'`，移动、最大化、启动用 `hyprctl dispatch 'hl.dsp...'`。`hyprctl output create headless` 建出的屏幕由 Omarchy 分配一组编号工作区（实测为 11），脚本给该工作区去掉外边距和边框，把所有浏览器窗口移过去并最大化；新窗口（登录页、弹窗）按 Omarchy 的 `on_focus_under_fullscreen` 接替。没有浏览器窗口时用 `launch_command` 启动，间隔不少于 `relaunch_seconds`。Omarchy 会把这块屏幕记入 `~/.local/state/omarchy/monitor-workspaces`。
- 顶栏：Omarchy 的 quickshell 给每块屏幕都建顶栏和 Dock，没有按屏幕排除的设置；全屏既盖不住顶栏，又会让 Chromium 进入自身全屏、藏起标签页和地址栏。所以脚本在屏幕保留区变化时上报 `{"reserved":[左,上,右,下]}`，页面把显示区域按保留区放大并平移，虚拟屏幕随之加大，顶栏落在可视区域之外。
- 画面：wayvnc 只投这块屏幕，监听客机回环的普通 VNC 端口（它自带的 WebSocket 在本镜像里一有连接就段错误）。同一条 SSH 用 `-L` 转到本机回环随机端口，Rust 连上后把收到的字节经 Tauri 二进制通道交给页面，页面里的通道对象实现 noVNC 所需的 WebSocket 形状，输入按顺序经 `browser_send` 写回。不开 WebSocket 服务，安全策略不变。
- 尺寸：页面按显示区域 CSS 尺寸乘取整后的设备像素比设置虚拟屏幕分辨率和缩放；区域变化时经 stdin 更新。
- 键盘：Mac 的 Command 在 noVNC 里是 Super（Omarchy 的窗口管理键），本页把它换成 Ctrl，复制、粘贴、新建/关闭标签页按浏览器习惯生效。中文用虚拟机里的输入法，或用本机复制、虚拟机粘贴。
- 剪贴板：不处理 noVNC 的剪贴板事件，虚拟机内容不回本机；本机文字仍经单向剪贴板进入虚拟机。
- 生命周期：SSH 断开时 stdin 关闭，脚本停止 wayvnc、取消全屏并移除虚拟屏幕，浏览器窗口回到虚拟机自己的桌面。关机、关闭软件时与对话连接一起断开。

## 单向剪贴板

本机复制的文字可以在虚拟机里粘贴，虚拟机的内容不回本机。没有使用 QEMU 的 vdagent：它是双向的，客机的 spice-vdagent 也不支持 Wayland。

软件为每次启动多开一条 virtio 串口 `vm.clipboard_port_name`。`clipboard::forward` 在虚拟机运行期间轮询本机剪贴板，文字变化时写入 4 字节长度加 UTF-8 内容，这条连接只写不读。客机的 `omarchy-space-clipboard` 服务以只读方式打开串口，用 `wl-copy` 写入账号的 Wayland 会话。参数在 `clipboard` 和 `guest.clipboard`，只传文字，超过 `max_bytes` 的内容跳过。服务在每次开机由配置程序写入。

## 系统镜像下载

软件包只带虚拟机运行时、固件和 sing-box。系统镜像由清单 `downloads` 给出固定版本的下载地址和字节数，首次打开时下载到数据目录的 `omarchy-image/`。一个文件可以分多段：每段先落成 `.partN`，用 HTTP Range 续传，字节数对上后再合并。下载不占用 `busy`，关闭软件不受阻，下次接着下。资源包目录里若已有镜像，就直接使用，不再下载。

## 持久状态

软件默认使用原 Tauri `app_local_data_dir`；Mac 为 `~/Library/Application Support/dev.omarchy.space/`，Windows 为当前用户本地应用数据目录下的 `dev.omarchy.space`。默认路径保持原样，用户已有的目录联接不被改写。

存放位置偏好保存在原默认目录的 `storage-location.json`，原子替换后才更新内存中的选择。下列数据文件都相对于当前所选根目录；离线镜像仍直接从用户选择的资源包读取。保存的自定义位置不可用时明确报错，不静默切回默认目录。

- `omarchy-image/`：下载的系统镜像，Windows 的内核与 initramfs 每次启动都要用。
- `vm/disk.img`：独立可写客机磁盘。
- `vm/efi-vars.fd`：Mac 客机 EFI 变量。
- `vm/machine.json`：资源规格、镜像来源与 `configured` 状态。
- `vm/delivery.json`：上一次成功的客机回执。
- `vm/runtime.log`：QEMU 运行日志，不含配置通道的数据。
- `deployment.json`：最后一次阶段，便于排查中断。不能仅依据旧回执判定新一次配置成功。

客机 sing-box 配置通过 stdin 校验后由 `systemd-creds --with-key=host` 加密保存，服务使用 `LoadCredentialEncrypted` 读取。重配前保留原加密凭据和服务文件，默认 Agent 文件也会保存原值。

Claude Code 由客机在节点核验通过后，以账号身份运行 `guest.claude_installer.url` 指向的官方脚本，流量经强制代理，失败按配置重试。官方安装产物是 `~/.local/bin/claude` 符号链接，有这个链接时换节点不重装；安装前移走 Omarchy 的 mise 占位脚本。

`storage.rs` 负责位置偏好、目录身份和禁止迁移的校验；任务入口与结束通过同一操作锁协调，下载、启动或配置进行中不能换路径。VM 任务持有启动时的数据目录快照。已有 vm/disk.img 或 machine.json 时不允许更换到其他目录，目标 vm 非空也拒绝覆盖。更换位置不会搬动旧下载文件。

Windows 空间查询通过 GetDiskFreeSpaceExW 对解析后的实际目录进行，避免把 C 盘上的目录联接误当成 C 盘容量。可选磁盘上限为所选卷可用 GiB 与产品最大值的较小值，创建前重新查询并在后端校验。最小磁盘由编译进客户端的 runtime-profiles.json 决定：Windows 16 GiB，Mac 32 GiB。RuntimeProfile 负责读取和校验，inspect_host 返回 runtime_profile.min_disk_gib，UI 输入/滑块/创建按钮和后端 start_install 使用同一平台值。外部资源包 Manifest 没有新增必需字段，旧离线包可继续使用。全局默认 64 GiB、最大 512 GiB 及创建前至少 20 GiB 可用空间仍在 product.config.json；vm::prepare 在扩容前继续拒绝小于实际出厂镜像的磁盘。

应用只管理单个环境；第二次打开客户端会聚焦现有窗口。运行代次用于丢弃旧安装线程在关机/重启后的迟到状态。

## 上游依据

- [systemd 临时 unit/drop-in 凭据](https://www.freedesktop.org/software/systemd/man/latest/systemd-debug-generator.html)
- [sing-box TUN](https://sing-box.sagernet.org/configuration/inbound/tun/)
- [Omarchy 默认 Agent 实现](https://github.com/omacom/omarchy/blob/quattro/bin/omarchy-default-agent)
- [Claude Code 安装说明](https://code.claude.com/docs/en/setup)
- [Try Omarchy](https://github.com/omacom/try-omarchy) 与 [Windows 运行时](https://github.com/omacom/try-omarchy-windows)

本版直接使用固定运行时参数，不调用尚未纳入当前固定资源包的 VM Kit，也不接入其宿主个人配置桥接。


## Windows 兼容显示

Windows runtime profile 的 software_graphics 定义 CPU 图形设备、SDL 2D 显示、内核渲染参数与子进程渲染器偏好。Machine.software_rendering 为可选持久字段，旧记录默认为 false，沿用 GPU 路径。set_software_rendering 只在任务空闲且 VM 已关闭时保存；inspect_host 返回该开关状态和平台是否提供此能力。

启动时仅覆盖一个图形设备、一个显示参数及内核渲染标记，保留其余 manifest 参数；旧离线 manifest 不新增必需字段。显示环境变量只传入 QEMU 子进程。此模式没有自动重试、迁移虚拟磁盘或改变强制代理的 -S/网卡控制流程。
