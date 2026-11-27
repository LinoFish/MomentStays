# Release 描述规则

- Release 标题默认使用英文，产品名使用 MomentStays；正文先写英文说明，再附对应中文翻译。
- 中英文内容保持一致，每种语言通常各用 1–5 条短句，不在翻译中追加功能或扩大兼容性承诺。
- 新 Release 的描述保持简洁，通常用 1–5 条短句。
- 只写本次发布的 App 本身的功能新增、使用体验变化和问题修复，以用户能感知的变化为准。
- 不写代码整合、提交、构建、测试过程、安装包设计、网站或文档维护等发布工作。
- 不重复列出已有功能、下载附件清单、系统要求、协议和常规签名声明。
- 若本次 App 存在必须告知的兼容性变化或已知问题，用一句话说明。
- 用户对某次发布另有明确要求时，优先遵循该次要求。

# Release 版本规则

- 每次发布都必须提升应用版本号和 build，不复用或覆盖已发布版本。
- 发布准备时核对远端最新 tag、Release 及当前代码版本，按本次变更范围确定递增后的版本；不能将旧任务中的版本号或示例锁定为后续发布版本。
- 确定版本后统一应用配置、构建产物、tag 和 Release，并在最终构建前冻结；发布前再次确认目标版本未被占用。

# 多平台版本规则

- macOS、iOS、Linux（Flatpak）等所有客户端共用一个产品版本号，每次发布只打一个 tag（如 `v2.1.0`），对应一个 Release。
- build 号由各平台各自递增（如 macOS 的 `CFBundleVersion`、Flatpak AppStream 的 `<release>`），不要求各平台一致。
- 只改动单个平台时，提升补丁版本号，只发布该平台；Release 正文用一句话写明本次涉及的平台。
- 某平台在本次版本中没有变化时，可以不出新包；Release 里不为它补写说明。
- Rust 核心 crate 的版本号仅供内部使用，不必与产品版本一致。

# 依赖协议规则

- 本项目自身采用 GPL-3.0-only，版权人为 Lino（LinoFish）；但**任何第三方依赖、复制的代码片段、素材都不得是 GPL**。官方 Mac App Store、Microsoft Store、Flathub 版本由版权人以其他条款发布，混入他人 GPL 代码会让这些版本无法上架。
- 允许：MIT、Apache-2.0、BSD-2-Clause、BSD-3-Clause、ISC、Zlib、Unlicense、Unicode（Unicode-3.0 / Unicode-DFS-2016）、MPL-2.0、CC0-1.0；字体允许 OFL-1.1。
- 禁止：GPL（任何版本）、AGPL、SSPL、以捆绑或静态链接方式引入的 LGPL，以及其他会阻碍应用商店分发的 copyleft 协议（含 EUPL、OSL、CC-BY-SA 用于代码等）。不确定的协议一律按禁止处理，先问用户。
- LGPL 系统库例外：只有由操作系统或 Flatpak 运行时提供、并以动态链接方式使用的 LGPL 库（如 GTK、GStreamer）可以使用；不得把它们打包进 App 或静态链接。
- HEVC 等专利编解码优先使用操作系统自带的编解码器（AVFoundation / VideoToolbox、Windows Media Foundation、GStreamer 系统插件），不要自带 x265、FFmpeg、libde265 等实现；宽松协议不代表已解决专利授权。
- 每次新增或升级依赖：核对其及全部传递依赖的协议，Rust crate 须通过 `cargo deny check licenses`（配置见 `core/deny.toml`，CI 的 rust-core 工作流会执行，出现 GPL 等协议会直接失败）；新增依赖须登记到 `THIRD_PARTY_NOTICES.md`（名称、版本、协议、用途）。
- 不得修改 `core/deny.toml` 的允许列表来放行被禁止的协议；需要例外时先问用户。
- 外部贡献须同意 `.github/CLA.md`；名称与图标规则见 `TRADEMARKS.md`。
