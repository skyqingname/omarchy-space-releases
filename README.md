# Omarchy 隔离空间

在你的电脑里建一套独立的 Omarchy 虚拟机：系统、账号和文件都留在虚拟机里，默认 Agent 是 Claude Code，所有联网强制走你填入的节点。

**当前是 0.1.0 开发预览。** Mac 版已在真机验证，Windows 版尚未在真机运行过。详情见 [Releases](https://github.com/skyqingname/omarchy-space-releases/releases)。

## 下载

GitHub 限制单个发行文件小于 2 GiB，所以每个平台分成两个分卷，**两个都要下载**，放在同一个文件夹。

### Mac（Apple 芯片，macOS 15 及以上）

下载 `Omarchy-Space-0.1.0-macos-arm64.zip.001` 和 `.002`，在终端进入下载目录后运行：

```sh
cat Omarchy-Space-0.1.0-macos-arm64.zip.0* > Omarchy-Space.zip
ditto -x -k Omarchy-Space.zip /Applications
xattr -cr "/Applications/Omarchy 隔离空间.app"
```

这一版没有经过 Apple 公证，第三行用来解除下载隔离，否则系统会提示无法打开。

### Windows 10 / 11（x64）

下载 `Omarchy-Space-0.1.0-windows-x64.zip.001` 和 `.002`，用 7-Zip 打开 `.001` 解压；或者在“命令提示符”里合并后再解压：

```bat
copy /b Omarchy-Space-0.1.0-windows-x64.zip.001+Omarchy-Space-0.1.0-windows-x64.zip.002 Omarchy-Space.zip
```

解压后保持文件夹结构，运行 `OmarchySpace.exe`。需要先在“启用或关闭 Windows 功能”里打开“Windows 虚拟机监控程序平台”，详见包内 `使用说明.md`。若出现“Windows 已保护你的电脑”，点“更多信息 → 仍要运行”。

## 使用前准备

- 你自己的订阅链接或节点链接。虚拟机只走这里填的节点，节点失效时不会直连。
- 至少 20 GB 可用磁盘空间、4 GB 以上可分配内存，并开启硬件虚拟化。
- 安装 Claude Code 和使用 Claude 都需要联网，Claude 账号由你自己登录。

## 第三方组件

发行包内含以下组件，它们各自遵循原有许可证，未作修改：

- [Omarchy](https://github.com/omacom/omarchy)
- [Try Omarchy](https://github.com/omacom/try-omarchy) 与 [Try Omarchy Windows](https://github.com/omacom/try-omarchy-windows) 运行时，其中包含 [QEMU](https://www.qemu.org/)（GPL）
- [sing-box](https://github.com/SagerNet/sing-box)（GPL）
- Microsoft WebView2 运行时安装器（仅 Windows 包）
