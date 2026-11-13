# GoToCS

Go to Claude Space，原“Omarchy 隔离空间”。在电脑里创建独立的 Omarchy 虚拟机，配置 sing-box 和 Claude Code，并在软件内使用对话、终端与虚拟机浏览器。

**已发布的软件包为 2026-11-13 开发预览，应用版本 0.1.0。** [发行说明与验证范围](https://github.com/skyqingname/omarchy-space-releases/releases/tag/v0.1.0-20261113)。本仓库同时提供完整应用源码、构建脚本与测试；原发行包和历史标签继续保留。

本次安装包包含宿主文件附件、API Key 登录状态修复、Windows 启动参数长度修复及 DMIT 镜像源配置。旧版 ZIP 不会自动更新，需下载新包或按下方说明应用补丁。

## 下载

| 平台 / 用途 | 下载 |
| --- | --- |
| Apple Silicon Mac，macOS 15+ | [Mac 完整包](https://github.com/skyqingname/omarchy-space-releases/releases/download/v0.1.0-20261113/GoToCS-0.1.0-macos-arm64.zip) |
| Windows 10/11 x64 | [Windows 完整包](https://github.com/skyqingname/omarchy-space-releases/releases/download/v0.1.0-20261113/GoToCS-0.1.0-windows-x64.zip) |
| 已有 Windows 用户，更新程序和下载清单 | [Windows 最新兼容补丁](https://github.com/skyqingname/omarchy-space-releases/releases/download/v0.1.0-20261113/GoToCS-windows-compat-patch-20261113.zip) |
| 校验文件 | [SHA-256](https://github.com/skyqingname/omarchy-space-releases/releases/download/v0.1.0-20261113/SHA256SUMS-20261113.txt) |

完整客户端包含运行时，不包含系统镜像或个人数据。首次打开时按资源清单下载镜像（Mac 约 2.9 GB，Windows 约 2.1 GB），中断后可继续。Windows 镜像使用 DMIT，Mac 沿用官方 GitHub 源，详见 [镜像源说明](docs/image-mirror.md)。Intel Mac、Windows ARM 尚未支持。

## 安装与更新

Mac：解压后将 `GoToCS.app` 放入“应用程序”。更新前正常关闭虚拟机并退出旧版。此版未经过 Apple 公证，系统可能阻止直接打开；请先核对来源与校验值，再按 macOS 的安全提示处理。

Windows：完整解压后保持 `GoToCS.exe` 与 `resources` 相邻，再运行程序。需要开启硬件虚拟化与 Windows Hypervisor Platform，安装 WebView2；内置终端还需要 Windows 可选功能中的 OpenSSH 客户端（`ssh.exe`、`ssh-keygen.exe`）。此版没有 Windows 发行签名，具体流程见包内“使用说明.md”。

旧版 Windows 更新：正常关闭虚拟机并退出软件，备份原 EXE 和 `resources/runtime-pack/manifest.json`，将兼容补丁中的 `OmarchySpace.exe` 放回原位置。若原文件名已为 `GoToCS.exe`，将补丁 EXE 改成这个名字再替换；同时复制补丁内的下载清单到对应位置。补丁适用于已有 v0.13.0 Windows 运行资源，其他版本请使用完整包。保留其余 `resources`、镜像和数据目录，不需要新建环境；原存放位置与“兼容显示”选择继续使用。不要同时运行新旧 EXE。

## 使用

1. 选择环境存放位置，下载系统镜像。
2. 输入自己的订阅或节点链接，选择节点并分配资源。
3. 创建环境，在虚拟机里完成自己的账号设置，按提示重启核验。
4. 登录自己的 Claude 账号，在“对话”使用 Claude Code，需要时切到“终端”或“浏览器”。

创建前至少需要 20 GiB 可用空间，磁盘最小值为 Windows 16 GiB、Mac 32 GiB；实际需求还取决于所选资源与镜像下载。Windows 图形初始化失败时，可在关机状态下于“我的环境”开启“兼容显示”，使用 CPU 绘制桌面。

节点来自软件里输入的链接；代理停止或节点失效时，客机应用不会回退直连。软件不导入宿主的 Claude 登录、浏览器资料或个人代理配置。下载系统镜像、安装及使用 Claude 均需要网络。

Claude Code 默认在虚拟机内以完全访问模式运行。“允许虚拟机访问本机”是独立开关，默认关闭；开启后可通过 SSH 操作宿主机，权限与宿主当前用户相同。Windows 可通过一次 UAC 准备仅监听本机回环地址的 SSH 服务。隔离措施不能阻止有客机管理员权限的人主动修改网络保护。

## 本机文件附件

在环境就绪后的「对话」输入框旁点击「添加附件」，可多选本机文件；也可把文件拖到 GoToCS 的对话页。文件以原始字节传入虚拟机工作目录下的 `attachments/<上传编号>/`，上传成功后随下一条消息提供客机文件路径。支持中文文件名、进度、取消、失败重试和附件移除；相同文件名不会覆盖已有副本。默认单文件上限 512 MiB、每条消息最多 20 个附件，参数在 `product.config.json.attachments`。

此功能不需要打开「允许虚拟机访问本机」。选择文件后立即开始传输，点击发送后 Claude 才收到附件路径。移除附件只移除消息引用，已传入的副本仍保留；未发送的附件列表不跨应用重启保存。文件夹请先压缩；DOCX、PDF 等可原样传入，内容解析仍取决于客机工具和 Claude 的格式支持，扫描件可能需要 OCR。拖入区域是 GoToCS 的对话页，并非外部 QEMU 桌面窗口。

2026-11-13 安装包已包含附件功能；此前的发行包没有此入口。接收协议及验证方式见 [附件传输说明](docs/attachments.md)。

## 验证边界

2026-11-12 的 Windows 本地源码构建已实际验证对话、API Key 状态、附件按钮、客机接收与 DOCX/PDF/TXT 读取；文件大小及 SHA-256 已独立核对。Windows 跨窗口物理拖放和新增功能的 macOS 实机验收仍未完成。从目录“拒绝访问”到此次功能验收的故障、处理和遗留问题见 [Windows 排障与验收记录](docs/windows-troubleshooting.md)。此记录不代表旧发行包已经更新。

本次公开包通过前端、Mac/Windows release 构建及专项测试，具体结果见发行说明。已有 Mac 版本曾验证启动、SSH 终端和虚拟机浏览器；本次新增功能的 Mac 实机、Windows 跨窗口拖放及真实 SSH 中断等场景仍待验收。本次 Windows 包为交叉构建，未把贡献者之前的源码实机测试视为此安装包的逐项验收。

## 源码与构建

| 目录 / 文件 | 内容 |
| --- | --- |
| `src/` | React/TypeScript 安装向导、对话、终端和浏览器界面 |
| `src-tauri/src/` | Rust 主机检测、存储、下载、QEMU 生命周期、SSH 和桌面命令 |
| `guest/` | Python 客机网络保护、环境配置、地区策略、对话跟随和浏览器脚本 |
| `scripts/` | 资源获取、打包、签名和 Windows SSH 配置 |
| `tests/` | Python / PowerShell 专项测试；Rust 测试与模块放在一起 |
| `product.config.json` / `runtime-profiles.json` | 产品和平台参数 |
| `asset-sources.json` / `build-assets.example.json` | 构建依赖地址和本机路径模板 |

从 [构建指南](docs/build.md) 开始。前端可用 `npm ci && npm run build` 验证；运行或分发桌面程序前，还需要准备平台运行资源。实现细节见 [架构与协议](docs/architecture.md)，Windows 使用方式见 [开发预览说明](docs/windows-preview.md)。

依赖锁文件和应用图标均已包含。源码仓库不包含个人配置、凭据、VM 数据、调试日志、`node_modules`、编译目录或第三方运行时大文件；`build-assets.local.json` 由使用者根据模板填写并保留在本地。

## 第三方组件

第三方组件按各自许可证分发：

- [Omarchy](https://github.com/omacom/omarchy)
- [Try Omarchy](https://github.com/omacom/try-omarchy) / [Try Omarchy Windows](https://github.com/omacom/try-omarchy-windows)，其中包含 [QEMU](https://www.qemu.org/)（GPL）
- [sing-box](https://github.com/SagerNet/sing-box)（GPL）
- [xterm.js](https://github.com/xtermjs/xterm.js)、[noVNC](https://github.com/novnc/noVNC)
