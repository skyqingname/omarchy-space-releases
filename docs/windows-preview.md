# GoToCS · Windows 开发预览（2026-11-10）

GoToCS（Go to Claude Space）原名“Omarchy 隔离空间”。此包包含当前对话、终端、虚拟机浏览器及兼容显示功能，仍为 0.1.0 开发预览。应用标识 dev.omarchy.space 保持不变，原环境及存放位置继续使用。

## 首次使用

1. 完整解压 GoToCS-0.1.0-windows-x64.zip，保持 GoToCS.exe 与 resources 文件夹相邻，再运行 GoToCS.exe。
2. 使用 Windows 10/11 x64，开启硬件虚拟化及 Windows Hypervisor Platform，安装 WebView2。内置终端使用 Windows OpenSSH 客户端，需要系统能运行 ssh.exe 和 ssh-keygen.exe；可在 Windows 可选功能中安装“OpenSSH 客户端”。
3. 按向导选择存放位置、下载镜像、输入自己的订阅/节点并分配资源。Windows 虚拟磁盘最小 16 GiB；创建前至少 20 GiB 可用空间，磁盘上限按所选位置的可用量计算。
4. 在虚拟机桌面完成自己的账号设置，按提示重启核验。之后可在软件的“对话”里使用客机 Claude Code，或切换原始终端。登录由用户本人完成。
5. “浏览器”页显示的是虚拟机中的 Chromium；“打开桌面”显示平时隐藏的完整虚拟机桌面。

首次下载系统镜像约 2.1 GB；下载镜像、安装客机组件、Claude Code 和使用 Claude 服务均需要网络。完整客户端包不包含已创建的虚拟磁盘、用户登录或节点。

## DMIT 镜像下载源（2026-11-11）

Windows v0.13.0 的三个官方镜像已镜像到 DMIT，内容保持原样。已有完整客户端使用 `GoToCS-windows-dmit-source-20261111.zip`，退出软件后备份并替换 `resources/runtime-pack/manifest.json`；此补丁没有 EXE/运行组件，不能独立运行。已有镜像和下载断点保留。

首次使用可完整解压 `GoToCS-0.1.0-windows-x64-dmit-20261111.zip`。该包沿用 2026-11-10 同一个 EXE，只更换镜像下载清单并增加说明。原 GitHub 发行附件仍使用此前清单。地址、验证及回滚见 [image-mirror.md](image-mirror.md)。11 月 11 日提及的配置补丁/完整包为当时本地交付物，未作为本仓库的发行附件上传；已有用户可按镜像源说明更新清单。

## 旧版兼容补丁

GoToCS-windows-compat-patch-20261110.zip 仅含 OmarchySpace.exe，兼容原来的安装目录：

1. 在旧程序中正常关闭虚拟机，再退出程序。
2. 将原 OmarchySpace.exe 改名备份。
3. 将补丁中的 OmarchySpace.exe 放在原位置，保留 resources、镜像及数据目录。
4. 运行新 EXE，窗口名称显示为 GoToCS。无需新建环境或移动磁盘。

如果原文件名已经是 GoToCS.exe，将补丁中的 EXE 改名为 GoToCS.exe 后替换即可。不要同时运行旧、新 EXE。首次启动会按新版流程更新必要的客机配置，并保留原环境数据。

## 显示、权限与网络

- 图形初始化失败时，先关闭虚拟机，在“我的环境”开启“兼容显示”，再启动。它使用 CPU 绘制桌面，性能可能下降；原选择会继续保留。
- Claude Code 默认以完全访问模式在虚拟机内运行。访问 Windows 宿主机是独立开关，默认关闭；开启时可以通过一次 UAC 准备本机 SSH 服务，服务仅监听 127.0.0.1。关闭开关撤销本软件公钥，不卸载系统组件。
- 客机应用流量强制经过所选节点；代理停止或节点失败不回退直连。软件不导入宿主 Claude 登录或浏览器资料。

## 验证范围

Mac 已验证当前应用启动、终端连接和虚拟机浏览器等路径。对话钩子与记录处理已在 Linux 的真实交互式 Claude Code 验证，界面用真实事件录制回放测试。当前对话完整往返和所有卡片未在已登录的真实 Mac 应用中逐项验收。

Windows 新版已交叉构建并检查压缩包；ConPTY/SSH、内置浏览器、完整对话和硬件图形兼容性仍待 Windows 实机验收。程序没有 Windows 发行签名，属于开发预览。

出错时保留完整提示、vm/runtime.log、deployment.json 和失败阶段。旧环境默认位于用户本地应用数据目录 dev.omarchy.space，选择过其他存放位置的以原设置为准。不要为了排障删除磁盘或重新初始化环境。
