# 本机文件传入虚拟机

## 用户流程

环境就绪后，在「对话」中用输入框旁的附件按钮选择多个文件，或将文件拖到对话页。选择后立即上传，附件卡片显示排队、进度、客机确认、成功或错误；失败可重试，上传中可取消。所有附件成功或移除后才能发送，空消息加附件会发送「请读取这些附件」。消息只包含客机路径，不包含本机完整路径或文件的 Base64 内容。

文件按选择时的会话归属，切换历史会话不会把迟到的上传结果加到其他草稿。发送失败时保留正文和附件；成功后只移除本次发送的附件。附件与 `/`、`!` 命令不混发，避免文件路径变成命令参数。移除已经传好的附件不会删除客机文件。应用重启不恢复未发送的附件卡片。

各格式按原始字节传输，无扩展名过滤；DOCX/PDF 的解析、加密文档解密、扫描件 OCR 不属于传输协议。此变更没有给外部 QEMU 窗口实现拖放，也不改变虚拟机浏览器的文件选择器。

## 协议和边界

- `import_attachment` 接收上传 UUID、本机绝对文件路径及进度 Channel；只在已有环境 ready 且无启动任务时连接。宿主逐块读取普通文件，不将整个文件放入 JS 或 Rust 内存。
- 复用 `terminal::connection`、SSH key、known_hosts 和端口转发。无需宿主 SSH 服务、共享目录、新的网络监听端口或调整网络保护。
- SSH 运行内嵌 `guest/file_receive.py`，程序和 JSON 参数经现有 shell quoting 传递；原文件字节走 SSH stdin，不送入 Claude 终端。
- 客机逐级使用目录描述符和 `O_NOFOLLOW` 打开目录，拒绝绝对路径、父目录跳转、目录符号链接及不安全文件名。每次传输创建一个 UUID 子目录，暂存文件权限为 `0600`。精确接收声明长度并确认 EOF、fsync 后，以不能覆盖已有文件的 hard link 发布最终文件，再返回大小及 SHA-256。
- 宿主检查回执 UUID、名称和大小；SSH 提供传输完整性，SHA-256 回执可供进一步核对。进度达到 100% 只代表已写入 SSH，在客机成功回执前不显示成功。
- 宿主同时只运行一个上传。取消、虚拟机运行代次改变或超时会终止 SSH 并回收进程；EOF/接收错误会清除临时文件。主机/虚拟机强制掉电可能留下 `.partial`，不会被当作成功附件或覆盖用户文件。
- 配额、分块大小、目录和超时来自 `product.config.json.attachments`。当前默认 512 MiB/文件、20 个附件/消息、64 KiB 分块、10 分钟超时。不上传文件夹，也不自动递归读取本机目录。

## 检查

```sh
npm ci
npm run build
node --test tests/test_attachment_model.mjs
python3 -m unittest discover -s tests -p test_file_receive.py -v
python3 -m unittest discover -s tests -p test_transcript_auth.py -v
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Node 测试需要支持 TypeScript 类型擦除的 Node（例如 24+）；项目原构建指南使用 Node 26。接收端的目录描述符测试必须在 Linux/POSIX 执行，Windows 会明确跳过，不能据此报告完整接收测试通过。Rust 检查需要构建指南里的工具链和运行资源。

实机验收应使用不含个人信息的 DOCX、PDF、中文/带空格文件名、空文件、多文件和超限文件：选择和拖放各做一次，核对客机大小及 SHA-256；让 Claude 根据消息中的客机路径读取内容。另测取消、断开 SSH、关机、发送失败后重试，以及上传期间切换会话。构建成功不能替代这些设备验收。

## Windows 重建兼容

API Key 配置也参与图形输入框的登录状态判断；仅有 API 地址不算登录，首次引导和工作目录信任仍分别检查。Claude 和状态跟随进程都经 SSH 在客机内使用 `bash -lc` 启动，因此客机 `.bash_profile` 等登录脚本中导出的凭据也能被识别。登录脚本的普通输出进入诊断流，不混入状态 JSON。登录环境变更需重新连接对话才会生效。

检测读取的是客机环境及客机设置，只返回登录、引导和信任状态，不输出密钥，也不读取或转发本机环境变量、Claude 登录或设置文件。此修复不改变已有剪贴板、手动附件和“允许虚拟机访问本机”开关。Unix Rust 回归会在合成客机 HOME 中运行实际启动命令及跟随脚本，覆盖 API Key、Token、登录脚本输出和无凭据场景；不需要真实账号或调用模型。

Windows 启动时，较长的 provisioning OEM 字符串写到虚拟机目录的 `.smbios` 文件，通过 QEMU `-smbios type=11,path=...` 传入，避免整段脚本占用 Windows 命令行长度。文件保留原有 systemd credential 内容；网络保护、启动暂停和验证流程不变。macOS 保留原有内联参数方式。
