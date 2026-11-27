# Contributing / 参与贡献

## English

Thanks for helping improve MomentStays. Bug reports with the source device, format, conversion direction and app version are the most useful contribution; see [Feedback](README.md#feedback).

**License.** The project is licensed under [GPL-3.0-only](LICENSE). The names and icon are covered by [TRADEMARKS.md](TRADEMARKS.md), not the GPL.

**Contributor License Agreement (CLA).** Before a pull request can be merged, you must agree to the [CLA](.github/CLA.md). In short: you keep your copyright, and you grant LinoFish a perpetual, irrevocable license to use, relicense and distribute your contribution under any terms, including in app stores (Mac App Store, Microsoft Store, Flathub) under terms other than the GPL. This lets the official store builds include community contributions. The CLA may be collected with [CLA Assistant](https://github.com/cla-assistant/cla-assistant); until it is enabled, state in your pull request: "I have read and agree to the MomentStays CLA (.github/CLA.md)."

**Dependencies.** Do not add code or dependencies under GPL, AGPL, SSPL or bundled/statically linked LGPL, even though the project itself is GPL: such code would prevent the official app-store builds. Allowed licenses and the review process are in the 「依赖协议规则」 section of [AGENTS.md](AGENTS.md). Rust crates are checked by `cargo deny check licenses` (`core/deny.toml`) in CI, and every new dependency must be recorded in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

**Samples.** Do not attach personal photos or videos; strip sensitive metadata from any sample first.

## 中文

感谢你帮助改进 MomentStays。最有用的贡献是附上来源设备、格式、转换方向和 App 版本的问题反馈，见 [README](README.zh-CN.md)。

**协议**：本项目采用 [GPL-3.0-only](LICENSE)。名称与图标不随 GPL 授权，见 [TRADEMARKS.md](TRADEMARKS.md)。

**贡献者许可协议（CLA）**：拉取请求合并前，须同意 [CLA](.github/CLA.md)。简言之：你保留著作权，同时授予 LinoFish 一项永久、不可撤销的许可，可按任何条款使用、重新授权和分发你的贡献，包括以 GPL 以外的条款在应用商店（Mac App Store、Microsoft Store、Flathub）中发布。这样官方商店版本才能包含社区贡献。CLA 可通过 [CLA Assistant](https://github.com/cla-assistant/cla-assistant) 签署；在其启用前，请在拉取请求中写明：“I have read and agree to the MomentStays CLA (.github/CLA.md).”

**依赖**：即使本项目本身是 GPL，也不要引入 GPL、AGPL、SSPL 或捆绑/静态链接的 LGPL 代码或依赖，否则会阻碍官方应用商店版本。允许的协议与审查流程见 [AGENTS.md](AGENTS.md)「依赖协议规则」。CI 会用 `cargo deny check licenses`（`core/deny.toml`）检查 Rust crate；每个新依赖都必须登记到 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

**样片**：不要附带个人照片或视频；提交样片前先去除敏感元数据。
