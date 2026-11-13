# GoToCS · Windows 开发预览（2026-11-13）

GoToCS（Go to Claude Space）原名“Omarchy 隔离空间”。此包为 `v0.1.0-20261113`，应用版本仍为 0.1.0。包含对话、终端、虚拟机浏览器、兼容显示，以及本机文件附件、API 凭据状态识别和启动参数长度修复。应用标识 `dev.omarchy.space` 保持不变，原环境及存放位置继续使用。

## 首次使用

1. 完整解压 GoToCS-0.1.0-windows-x64.zip，保持 GoToCS.exe 与 resources 文件夹相邻，再运行 GoToCS.exe。
2. 使用 Windows 10/11 x64，开启硬件虚拟化及 Windows Hypervisor Platform，安装 WebView2。内置终端使用 Windows OpenSSH 客户端，需要系统能运行 ssh.exe 和 ssh-keygen.exe；可在 Windows 可选功能中安装“OpenSSH 客户端”。
3. 按向导选择存放位置、下载镜像、输入自己的订阅/节点并分配资源。Windows 虚拟磁盘最小 16 GiB；创建前至少 20 GiB 可用空间，磁盘上限按所选位置的可用量计算。
4. 在虚拟机桌面完成自己的账号设置，按提示重启核验。之后可在软件的“对话”里使用客机 Claude Code，或切换原始终端。登录由用户本人完成。
5. “浏览器”页显示的是虚拟机中的 Chromium；“打开桌面”显示平时隐藏的完整虚拟机桌面。

首次下载系统镜像约 2.1 GB；下载镜像、安装客机组件、Claude Code 和使用 Claude 服务均需要网络。完整客户端包不包含已创建的虚拟磁盘、用户登录或节点。

## DMIT 镜像下载源

Windows v0.13.0 的三个官方镜像已镜像到 DMIT，内容保持原样。本次完整包及兼容补丁均包含当前 `resources/runtime-pack/manifest.json`，旧版安装包不会自动切源。已有镜像和下载断点保留。

首次使用请下载 `GoToCS-0.1.0-windows-x64.zip`。2026-11-10 及更早发行附件保持原样；历史 DMIT 配置补丁只更新清单，不包含本次程序修复。地址、验证及回滚见 [image-mirror.md](image-mirror.md)。

## 旧版兼容补丁

`GoToCS-windows-compat-patch-20261113.zip` 包含 `OmarchySpace.exe`、下载清单及使用说明，适用于已有 v0.13.0 Windows 运行资源的安装目录：

1. 在旧程序中正常关闭虚拟机，再退出程序。
2. 备份原 `OmarchySpace.exe`（或 `GoToCS.exe`）和 `resources/runtime-pack/manifest.json`。
3. 将补丁中的 `OmarchySpace.exe` 放在原位置，并将补丁内的 `resources/runtime-pack/manifest.json` 复制到对应位置；其余 resources、镜像及数据目录保留。
4. 运行新 EXE，窗口名称显示为 GoToCS。无需新建环境或移动磁盘。

如果原文件名已经是 GoToCS.exe，将补丁中的 EXE 改名为 GoToCS.exe 后替换即可。不要同时运行旧、新 EXE。首次启动会按新版流程更新必要的客机配置，并保留原环境数据。

补丁不含完整运行时，不能独立运行。运行资源版本不同或文件不完整时，使用完整包并继续选择原数据目录，不要删除或重建虚拟磁盘。回滚时正常关闭程序，恢复备份的 EXE 和下载清单。

## 文件附件与 API 凭据

环境就绪后，在对话输入框点击附件按钮多选本机文件，上传完成后发送；默认最多 20 个附件、单文件 512 MiB。失败可重试，上传中可取消。只传所选文件，不需要开启“允许虚拟机访问本机”。已传入副本位于客机工作目录的 attachments 子目录，移除卡片不会删除副本。Windows 资源管理器跨窗口物理拖放仍待验收。

图形登录状态识别客机 API Key/Token，Claude 与检测进程均在客机登录 shell 中启动；不导入本机 Claude 登录、设置或环境变量。首次引导和工作目录信任仍需在客机完成。修改客机登录脚本后需重新连接对话。

## 显示、权限与网络

- 图形初始化失败时，先关闭虚拟机，在“我的环境”开启“兼容显示”，再启动。它使用 CPU 绘制桌面，性能可能下降；原选择会继续保留。
- Claude Code 默认以完全访问模式在虚拟机内运行。访问 Windows 宿主机是独立开关，默认关闭；开启时可以通过一次 UAC 准备本机 SSH 服务，服务仅监听 127.0.0.1。关闭开关撤销本软件公钥，不卸载系统组件。
- 客机应用流量强制经过所选节点；代理停止或节点失败不回退直连。软件不导入宿主 Claude 登录或浏览器资料。

## 验证范围

本次构建验证、专项测试及压缩包校验见发行页。2026-11-12 贡献者曾在 Windows 源码构建中验证真实对话、附件按钮、DOCX/PDF/TXT 读取及大小/哈希，但这不替代本次交叉构建安装包的完整设备验收。

Windows 跨窗口物理拖放、真实 SSH 中断/超时/上传中关机及硬件图形兼容性仍待验收。部分镜像的 ufw/Apps 依赖、锁屏灰屏、TXT 默认打开和显示语言体验仍有已知问题，详见源码仓库的 `docs/windows-troubleshooting.md`。程序没有 Windows 发行签名，属于开发预览。

出错时保留完整提示、vm/runtime.log、deployment.json 和失败阶段。旧环境默认位于用户本地应用数据目录 dev.omarchy.space，选择过其他存放位置的以原设置为准。不要为了排障删除磁盘或重新初始化环境。
