# GoToCS

Go to Claude Space，原“Omarchy 隔离空间”。在电脑里创建独立的 Omarchy 虚拟机，配置 sing-box 和 Claude Code，并在软件内使用对话、终端与虚拟机浏览器。

**当前为 2026-11-10 开发预览，应用版本 0.1.0。** [发行说明与验证范围](https://github.com/skyqingname/omarchy-space-releases/releases/tag/v0.1.0-20261110)。这个仓库用于分发软件包和使用说明。

## 下载

| 平台 / 用途 | 下载 |
| --- | --- |
| Apple Silicon Mac，macOS 15+ | [Mac 完整包](https://github.com/skyqingname/omarchy-space-releases/releases/download/v0.1.0-20261110/GoToCS-0.1.0-macos-arm64.zip) |
| Windows 10/11 x64 | [Windows 完整包](https://github.com/skyqingname/omarchy-space-releases/releases/download/v0.1.0-20261110/GoToCS-0.1.0-windows-x64.zip) |
| 已有 Windows 用户，只替换程序 | [Windows 最新兼容补丁](https://github.com/skyqingname/omarchy-space-releases/releases/download/v0.1.0-20261110/GoToCS-windows-compat-patch-20261110.zip) |
| 校验文件 | [SHA-256](https://github.com/skyqingname/omarchy-space-releases/releases/download/v0.1.0-20261110/SHA256SUMS-20261110.txt) |

完整客户端包含运行时，不包含系统镜像或个人数据。首次打开时从官方发行页下载镜像（Mac 约 2.9 GB，Windows 约 2.1 GB），中断后可继续。Intel Mac、Windows ARM 尚未支持。

## 安装与更新

Mac：解压后将 `GoToCS.app` 放入“应用程序”。更新前正常关闭虚拟机并退出旧版。此版未经过 Apple 公证，系统可能阻止直接打开；请先核对来源与校验值，再按 macOS 的安全提示处理。

Windows：完整解压后保持 `GoToCS.exe` 与 `resources` 相邻，再运行程序。需要开启硬件虚拟化与 Windows Hypervisor Platform，安装 WebView2；内置终端还需要 Windows 可选功能中的 OpenSSH 客户端（`ssh.exe`、`ssh-keygen.exe`）。此版没有 Windows 发行签名，具体流程见包内“使用说明.md”。

旧版 Windows 只更新程序：正常关闭虚拟机并退出软件，备份原 EXE，将兼容补丁中的 `OmarchySpace.exe` 放回原位置。若原文件名已为 `GoToCS.exe`，将补丁 EXE 改成这个名字再替换。保留 `resources`、镜像和数据目录，不需要新建环境；原存放位置与“兼容显示”选择继续使用。不要同时运行新旧 EXE。

## 使用

1. 选择环境存放位置，下载系统镜像。
2. 输入自己的订阅或节点链接，选择节点并分配资源。
3. 创建环境，在虚拟机里完成自己的账号设置，按提示重启核验。
4. 登录自己的 Claude 账号，在“对话”使用 Claude Code，需要时切到“终端”或“浏览器”。

创建前至少需要 20 GiB 可用空间，磁盘最小值为 Windows 16 GiB、Mac 32 GiB；实际需求还取决于所选资源与镜像下载。Windows 图形初始化失败时，可在关机状态下于“我的环境”开启“兼容显示”，使用 CPU 绘制桌面。

节点来自软件里输入的链接；代理停止或节点失效时，客机应用不会回退直连。软件不导入宿主的 Claude 登录、浏览器资料或个人代理配置。下载系统镜像、安装及使用 Claude 均需要网络。

Claude Code 默认在虚拟机内以完全访问模式运行。“允许虚拟机访问本机”是独立开关，默认关闭；开启后可通过 SSH 操作宿主机，权限与宿主当前用户相同。Windows 可通过一次 UAC 准备仅监听本机回环地址的 SSH 服务。隔离措施不能阻止有客机管理员权限的人主动修改网络保护。

## 验证边界

Mac 已验证启动、SSH 终端和虚拟机浏览器。对话钩子在真实 Linux Claude Code 会话中验证，界面以真实事件回放验证；已登录 Mac 应用中的完整对话往返及所有卡片尚未逐项验收。Windows 当前包已交叉构建并校验，新版终端、浏览器、对话、兼容显示与宿主 SSH 功能仍待 Windows 实机验收。

## 第三方组件

第三方组件按各自许可证分发：

- [Omarchy](https://github.com/omacom/omarchy)
- [Try Omarchy](https://github.com/omacom/try-omarchy) / [Try Omarchy Windows](https://github.com/omacom/try-omarchy-windows)，其中包含 [QEMU](https://www.qemu.org/)（GPL）
- [sing-box](https://github.com/SagerNet/sing-box)（GPL）
- [xterm.js](https://github.com/xtermjs/xterm.js)、[noVNC](https://github.com/novnc/noVNC)
