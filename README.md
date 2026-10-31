# Omarchy 隔离空间

在你的电脑里建一套独立的 Omarchy 虚拟机：系统、账号和文件都留在虚拟机里，默认 Agent 是 Claude Code，所有联网强制走你填入的节点。

**当前是 0.1.0 开发预览。** Mac 版已在真机验证，Windows 版尚未在真机运行过。详情见 [Releases](https://github.com/skyqingname/omarchy-space-releases/releases)。

## 下载

到 [Releases](https://github.com/skyqingname/omarchy-space-releases/releases/tag/v0.1.0) 页面底部的 **Assets** 下载对应平台的压缩包。软件只有几十 MB，首次打开时会从 Omarchy 官方发行页下载系统镜像（Mac 约 2.9 GB，Windows 约 2.1 GB），中断后可以接着下。

### Mac（Apple 芯片，macOS 15 及以上）

下载 `Omarchy-Space-0.1.0-macos-arm64.zip`，在终端进入下载目录后运行：

```sh
ditto -x -k Omarchy-Space-0.1.0-macos-arm64.zip /Applications
xattr -cr "/Applications/Omarchy 隔离空间.app"
```

这一版没有经过 Apple 公证，第二行用来解除下载隔离，否则系统会提示无法打开。

### Windows 10 / 11（x64）

下载 `Omarchy-Space-0.1.0-windows-x64.zip`，解压后保持文件夹结构，运行 `OmarchySpace.exe`。需要先在“启用或关闭 Windows 功能”里打开“Windows 虚拟机监控程序平台”，详见包内 `使用说明.md`。若出现“Windows 已保护你的电脑”，点“更多信息 → 仍要运行”。

## 使用前准备

- 你自己的订阅链接或节点链接。虚拟机只走这里填的节点，节点失效时不会直连。
- 至少 25 GB 可用磁盘空间、4 GB 以上可分配内存，并开启硬件虚拟化。
- 下载系统镜像、安装 Claude Code 和使用 Claude 都需要联网，Claude 账号由你自己登录。

本机复制的文字可以在虚拟机里粘贴（Mac 上是 ⌘V）。

“允许虚拟机访问本机”默认关闭。打开后，虚拟机里的 Claude Code 可以用 `ssh host` 操作这台电脑，权限和你一样，虚拟机也就不再是隔离环境。Mac 需要先打开“远程登录”，Windows 需要安装 OpenSSH 服务器。

## 第三方组件

软件包内含以下组件，系统镜像首次打开时下载，它们各自遵循原有许可证，未作修改：

- [Omarchy](https://github.com/omacom/omarchy)
- [Try Omarchy](https://github.com/omacom/try-omarchy) 与 [Try Omarchy Windows](https://github.com/omacom/try-omarchy-windows) 的系统镜像和运行时，其中包含 [QEMU](https://www.qemu.org/)（GPL）
- [sing-box](https://github.com/SagerNet/sing-box)（GPL）
