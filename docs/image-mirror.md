# Windows 镜像下载源

当前源码将 Try Omarchy Windows v0.13.0 的三个官方镜像配置为从 DMIT 下载。镜像内容与上游一致，Mac 仍使用官方 GitHub 下载源。

| 文件 | 字节数 |
| --- | ---: |
| [rootfs.ext4.zst](https://download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/rootfs.ext4.zst) | 1,877,333,998 |
| [initramfs-linux.img](https://download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/initramfs-linux.img) | 360,175,920 |
| [vmlinuz-linux](https://download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/vmlinuz-linux) | 17,244,672 |

同目录提供 [SHA256SUMS](https://download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/SHA256SUMS)、[SOURCE.json](https://download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/SOURCE.json) 与上游许可证。上游来源为 [官方 v0.13.0](https://github.com/omacom/try-omarchy-windows/releases/tag/v0.13.0)。

## 构建与已有客户端

构建真源是 `runtime-profiles.json` 的 `windows-x86_64.downloads`，资源准备脚本将其写入随包 `manifest.json`。修改下载地址后须重新生成资源清单；只重新编译 EXE 不会更新旧资源包。

已有 Windows 客户端读取 `resources/runtime-pack/manifest.json`。退出软件并备份清单后，可以只把三个文件对应的 URL 改为上述地址，保留文件名、字节数及所有运行参数。重开软件后生效。若使用手动选择的资源包，应修改实际使用的那个目录。

2026-11-13 完整包及兼容补丁已包含 DMIT 清单。2026-11-10 GitHub 发行附件保留原样，其镜像清单仍指向 GitHub；用户已有客户端不会自动切源。可升级到新包，或按上述方式更新清单。

## 完整性与续传

镜像发布时核对了官方完整文件的大小和 SHA-256；服务支持 HTTPS 与 HTTP Range。客户端沿用 `.partN` 断点文件，切源不改变文件内容和分段方式。现有下载器检查下载字节数，尚未在客户端增加 SHA-256 校验或自动切源。

不同网络访问 DMIT 与 GitHub 的速度可能不同，本项目不保证镜像源对所有地区都更快。恢复原 GitHub URL 或备份清单即可回滚，不需要删除已有下载或重新创建虚拟机。
