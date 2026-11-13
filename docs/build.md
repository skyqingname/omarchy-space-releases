# 从源码构建 GoToCS

仓库包含桌面前端、Rust 后端、客机脚本、测试、图标和构建脚本。运行时二进制、系统镜像、个人环境和本机路径配置不存入 Git；程序运行还需要按平台准备资源包。

## 工具链

依赖精确版本以 `package-lock.json`、`src-tauri/Cargo.lock` 为准。本次源码导入在 Node.js 26.5.0、npm 11.17.0、Rust/Cargo 1.99.1、Python 3.9.6 的 macOS 环境验证；这组版本是验证环境，不是声明的最低版本。Python 标准库用于构建和测试脚本。

macOS 需要 Apple Silicon、macOS 15+ 和 Xcode 命令行工具。Windows 原生构建需要 Rust MSVC 工具链、Visual Studio C++ Build Tools、Windows SDK、Node.js、Python 及 WebView2；脚本通过 PATH 调用 `curl`。在其他平台交叉构建 Windows 时，需要已有的 cargo-xwin/Windows SDK 工具链。

```sh
npm ci
npm run build
python3 -m unittest discover -s tests -v
```

`npm run build` 包含 TypeScript 检查和 Vite 打包。Tauri 构建脚本在 `cargo check` / Rust 单元测试阶段也检查随包资源，所以须先完成下方对应平台的资源准备；测试本身不需要运行虚拟机。宿主 SSH 用户名测试会调用 `/usr/bin/ssh`，上述 Python 测试命令在 macOS/Linux 执行。PowerShell 的 `tests/test_windows_host_ssh.ps1` 使用模拟 cmdlet 验证配置顺序，不替代真实 Windows UAC/安装验收。

## macOS 运行资源

使用 `runtime-profiles.json` 对应的 [Try Omarchy v1.2.0](https://github.com/omacom/try-omarchy/releases/tag/v1.2.0) 运行时、固件和匹配架构的 sing-box Linux 软件包。将以下模板复制为本机配置，填写实际文件位置：

```sh
cp build-assets.example.json build-assets.local.json
# 编辑 build-assets.local.json 中的 /path/to/... 路径。
python3 scripts/stage-assets.py build-assets.local.json
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm run tauri -- build --target aarch64-apple-darwin --bundles app
python3 scripts/sign-macos.py
python3 scripts/package-release.py
```

`stage-assets.py` 复制运行时、固件、sing-box 包及上游许可证，并从根平台配置生成 `src-tauri/resources/runtime-pack/manifest.json`。系统镜像由客户端首次运行时另行下载。当前打包配置使用本地 ad-hoc 签名；不包含 Apple 公证。开发运行使用 `npm run tauri -- dev`。

## Windows 运行资源

先按 `asset-sources.json` 获取固定版本 QEMU/WINQ-EMU、zstd 与 sing-box。脚本只准备构建目录，不安装宿主系统组件：

```sh
python3 scripts/fetch-windows-assets.py

# Windows 原生构建
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm run tauri -- build --target x86_64-pc-windows-msvc --no-bundle
python3 scripts/package-windows.py

# 已配置 cargo-xwin 的 macOS/Linux 开发机使用下面的构建命令替代上面那条：
# npm run tauri -- build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle
```

脚本生成 `assets/windows-x86_64/manifest.json`；Tauri 的 Windows 资源映射位于 `src-tauri/tauri.windows.conf.json`。可移植 ZIP 包含 EXE 与相邻 `resources/` 文件夹，两者必须一起分发。只复制 EXE 会导致资源检测失败。

当前源码及 2026-11-13 Windows 包的镜像地址为 DMIT，详情见 [镜像源说明](image-mirror.md)。2026-11-10 ZIP 早于该配置变更，旧附件保留原样。

## 参数、产物与数据

- `product.config.json`：产品默认值、网络/地区策略、安装和交互参数。
- `runtime-profiles.json`：平台 VM 参数、镜像地址/大小、兼容显示和最小磁盘。
- `asset-sources.json`：Windows 构建依赖地址。
- `packaging.config.json`：本机构建产物路径、打包名称和签名身份。
- `build-assets.local.json`：本机 macOS 运行时路径，已被 Git 忽略。

产物位于 `src-tauri/target/` 和 `dist-packages/`；测试夹具在被忽略的 `evidence/` 下生成。程序数据存放于用户应用数据目录或用户选择的位置，不属于源码。构建成功不等于安装、虚拟机启动、图形兼容、网络隔离和完整对话均通过实机验收。
