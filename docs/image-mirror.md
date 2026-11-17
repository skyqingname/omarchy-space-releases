# 系统镜像下载源

当前 Mac 和 Windows 系统镜像均从 DMIT 下载，文件保持官方原样。Mac 源自 Try Omarchy v1.2.0，Windows 源自 Try Omarchy Windows v0.13.0；切源没有升级或替换镜像内容。

## Mac · Apple Silicon

自 [v0.1.0-20261117.1](https://github.com/skyqingname/omarchy-space-releases/releases/tag/v0.1.0-20261117.1) 起，Mac 完整包使用以下三个分片，合计 3,166,644,370 字节（约 2.95 GiB）：

| 文件 | 字节数 |
| --- | ---: |
| [omarchy-aarch64-edge.img.zst.001](https://download.maqihang.si/releases/gotocs/images/macos-aarch64/v1.2.0/omarchy-aarch64-edge.img.zst.001) | 1,073,741,824 |
| [omarchy-aarch64-edge.img.zst.002](https://download.maqihang.si/releases/gotocs/images/macos-aarch64/v1.2.0/omarchy-aarch64-edge.img.zst.002) | 1,073,741,824 |
| [omarchy-aarch64-edge.img.zst.003](https://download.maqihang.si/releases/gotocs/images/macos-aarch64/v1.2.0/omarchy-aarch64-edge.img.zst.003) | 1,019,160,722 |

同目录提供 [SHA256SUMS](https://download.maqihang.si/releases/gotocs/images/macos-aarch64/v1.2.0/SHA256SUMS)、[SOURCE.json](https://download.maqihang.si/releases/gotocs/images/macos-aarch64/v1.2.0/SOURCE.json) 与上游许可证。三个镜像分片的完整 SHA-256 与 [官方 v1.2.0](https://github.com/omacom/try-omarchy/releases/tag/v1.2.0) 的资产 digest 一致。

更新前正常关闭虚拟机并退出旧程序，备份旧 app，再替换为新完整包。下载的 `factory.img.zst.part0`、`.part1`、`.part2` 名称和分片顺序保持不变，可继续已有下载进度；已下载好的镜像和虚拟磁盘无需重新创建。Mac app 中的清单属于签名资源，推荐替换完整 app，而非手改 app 内清单。

## Windows · x64

自 2026-11-13 完整包及兼容补丁起包含 DMIT 清单：

| 文件 | 字节数 |
| --- | ---: |
| [rootfs.ext4.zst](https://download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/rootfs.ext4.zst) | 1,877,333,998 |
| [initramfs-linux.img](https://download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/initramfs-linux.img) | 360,175,920 |
| [vmlinuz-linux](https://download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/vmlinuz-linux) | 17,244,672 |

同目录提供 [SHA256SUMS](https://download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/SHA256SUMS)、[SOURCE.json](https://download.maqihang.si/releases/gotocs/images/windows-x86_64/v0.13.0/SOURCE.json) 与上游许可证。上游为 [官方 v0.13.0](https://github.com/omacom/try-omarchy-windows/releases/tag/v0.13.0)。

已有 Windows 客户端读取 `resources/runtime-pack/manifest.json`。退出软件并备份清单后，可使用新完整包或兼容补丁；若只修改清单，保留文件名、字节数及所有运行参数。手动选择资源包时，应更新实际使用的那个资源包。2026-11-10 及更早发行附件保留原样，旧客户端不会自动切源。

## 构建、完整性与回滚

真源为 `runtime-profiles.json` 中各平台的 `downloads`；资源准备脚本生成随包清单。只重新编译程序不会自动更新旧资源包，发布时必须重新生成清单并检查包内值。

镜像发布前核对官方完整文件的大小和 SHA-256，服务支持 HTTPS 与 HTTP Range。客户端仍按分片顺序下载并使用 `.partN` 断点续传；只换源不改变文件内容和缓存布局。现有下载器检查字节数，尚未新增客户端 SHA-256 校验、自动切源、代理选项或并发下载。

镜像下载由宿主程序执行，不经过尚未创建的虚拟机节点。不同地区到 DMIT/GitHub 的速度不同，服务端同步速度或单台开发机测速不能替代客户链路验收。

需要回滚时，Mac 恢复备份 app，Windows 恢复旧清单或应用包；无需删除下载断点或虚拟磁盘。历史标签和附件保留，不覆盖固定版本镜像内容。
