# MomentStays GNOME 版 Flatpak 方案

状态：计划（尚未开始 GTK 界面）。核心前置工作见 `core/LINUX_NATIVE.md`（`--features linux-native` 编解码后端）与 `core/src/engine/decisions/`（macOS 与 GNOME 共用的界面决策）。

## 1. 分发方式：只发 Flatpak，运行时 `org.gnome.Platform//50`

- 只提供 Flatpak（Flathub），不做 deb/rpm/AppImage/Snap，避免多套系统库组合。
- 选 GNOME 运行时而非 `org.freedesktop.Platform`：GTK4、libadwaita、GStreamer、libheif、libjpeg-turbo、lcms2 都已在运行时内（GNOME 50 基于 freedesktop-sdk 25.08），无需自己打包这些 LGPL 库。
- HEVC/H.264/AAC 解码来自运行时自动安装的扩展 `org.freedesktop.Platform.codecs-extra`（FFmpeg 解码器、libde265/libx265、libheif HEVC 插件）。应用不捆绑任何编解码器。
- 协议：项目改为 GPL-3.0-only；LGPL 系统库（GTK、GStreamer、libheif 等）一律由运行时提供、动态链接；Rust 依赖仅允许 MIT/Apache-2.0/BSD/ISC/Zlib/MPL-2.0 等宽松协议（当前清单见 `core/LINUX_NATIVE.md` 与 `THIRD_PARTY_NOTICES.md`，CI 以 `cargo deny check licenses` 检查）。

## 2. 应用 ID

沿用 macOS 包标识 `com.linofish.momentstays` 的反向域名：**`com.linofish.MomentStays`**。
Flathub 要求能验证 `linofish.com` 域名所有权；若不能，改用 `io.github.LinoFish.MomentStays`（仓库托管于 GitHub，Flathub 自动认可）。

## 3. Manifest 概要（`com.linofish.MomentStays.json`）

```yaml
id: com.linofish.MomentStays
runtime: org.gnome.Platform
runtime-version: '50'
sdk: org.gnome.Sdk
sdk-extensions: [org.freedesktop.Sdk.Extension.rust-stable]
command: momentstays
finish-args:
  - --share=ipc
  - --socket=wayland
  - --socket=fallback-x11
  - --device=dri
build-options:
  append-path: /usr/lib/sdk/rust-stable/bin
  env: { CARGO_HOME: /run/build/momentstays/cargo }
modules:
  - name: momentstays
    buildsystem: simple
    build-commands:
      - cargo --offline build --release --features linux-native -p momentstays-gnome
      - install -Dm755 target/release/momentstays /app/bin/momentstays
      - install -Dm644 data/com.linofish.MomentStays.desktop -t /app/share/applications
      - install -Dm644 data/com.linofish.MomentStays.metainfo.xml -t /app/share/metainfo
      - install -Dm644 data/icons/... -t /app/share/icons/hicolor/...
    sources:
      - type: dir
        path: .
      - cargo-sources.json   # flatpak-cargo-generator.py Cargo.lock 生成
```

- Cargo 依赖用 `flatpak-cargo-generator` 生成 `cargo-sources.json` 离线 vendored（Flathub 构建无网络）。
- **不申请** `--filesystem=home/host`、`--socket=pulseaudio`（无需播放声音；音频只做解码校验）、`--share=network`。
- 文件访问全部经 xdg-desktop-portal：FileChooser（打开文件/文件夹、选择导出目录），Documents portal 持久化授权（`/run/user/$UID/doc/...`）。

## 4. 界面技术栈

- `gtk4-rs` + `libadwaita-rs`（MIT），Rust 直接调用 `momentstays-core`（同进程，不走 C ABI；C ABI 保留给 Swift）。
- 视频预览：GStreamer `gtk4paintablesink`（gst-plugins-rs，运行时自带）→ `GtkPicture`；逐帧取图用核心 `Native::frame_jpeg`。
- 本地化：gettext（`gettext-rs`），语言 zh-Hans（源文本）、zh-Hant、en。核心错误文本保持中文原文，界面按 `ErrorKind` 与 `hdr_kind` 等稳定代码翻译。

## 5. macOS 五步流程 → Adwaita 组件

| macOS 步骤 | GNOME 实现 |
|---|---|
| 1 转换方向 | `AdwNavigationSplitView` 侧栏步骤列表；方向用 `AdwPreferencesGroup` + `AdwActionRow`（单选），目标品牌用 `AdwComboRow` |
| 2 导入照片 | 拖放 + portal 文件/文件夹选择；网格 `GtkGridView`（缩略图），筛选 `GtkFilter`（实况/HDR/连拍/缺时间，按 `hdr_kind` 判断而非文案） |
| 3 选择转换特性 | `AdwPreferencesPage`：`AdwSwitchRow`（实况、HDR、连拍、时间补全），不可用原因用 `action-blocking-reason` 结果显示在 `AdwActionRow` subtitle |
| 4 导出预览 | `export-plan` 结果列表（`GtkListView`），封面/候选帧预览用 `gtk4paintablesink`；导出位置用 portal 选择目录 |
| 5 执行结果 | `GtkProgressBar` + 逐项 `GtkListView`；完成用 `AdwToast`，失败详情 `AdwAlertDialog`；可取消（`ms_stop` 等价的 `StopToken`） |

## 6. 来源：文件夹而非“照片”图库

- Linux 无系统照片库：来源是用户经 portal 授权的文件夹/文件（相机导入目录、手机传输目录等）。
- 文件夹变化用 GIO `GFileMonitor` 刷新（仅对已授权目录；Documents portal 下的 FUSE 路径监听能力有限，见风险）。
- 扫描/成组/配对逻辑全部在核心（`scan`、`live-selection`、`track-info` 等请求），与 macOS 一致。

## 7. AppStream / 桌面文件 / 图标

- `com.linofish.MomentStays.metainfo.xml`：名称、摘要（中英）、描述、截图（≥1，16:9）、`<releases>`、`<content_rating type="oars-1.1"/>`、`<developer id="com.linofish">`、`<url type="vcs-browser">`、`<launchable>`、`<branding>` 颜色；`appstreamcli validate --pedantic` 通过。
- `.desktop`：`Categories=Graphics;Photography;`，`MimeType=image/jpeg;image/heic;video/quicktime;video/mp4;`。
- 图标：`scalable/apps/com.linofish.MomentStays.svg` 与 `symbolic/apps/com.linofish.MomentStays-symbolic.svg`（由 `docs/branding/icon.svg` 派生，按 GNOME HIG 重绘）。

## 8. Flathub 提交清单

1. 应用 ID 域名验证（或改 `io.github.*`）。
2. Manifest 在 `flathub/flathub` 新建 PR（`new-pr` 分支），离线 vendored cargo 源。
3. finish-args 最小化并在 PR 说明每项理由；无 `--filesystem`。
4. metainfo / desktop / 图标通过 `flatpak-builder-lint`（manifest、builddir、repo 三项）。
5. 截图托管于固定 URL（仓库 tag 下的文件）。
6. 协议字段 `GPL-3.0-only`，与仓库一致；列出第三方协议（THIRD_PARTY_NOTICES）。
7. 发布说明遵守 `AGENTS.md` 规则（英文在前、中文翻译）。

## 9. CI

- 现有 `rust-core` 工作流新增 `linux-native` 任务（Ubuntu 系统库 + ffmpeg/heif-enc 合成样片 + 公开 iPhone 样片）。
- 新增 `flatpak` 工作流：容器 `ghcr.io/flathub-infra/flatpak-github-actions:gnome-50`，`flathub-infra/flatpak-github-actions/flatpak-builder@v6` 构建 x86_64 与 aarch64 bundle，再跑 `flatpak-builder-lint`；产物上传为 artifact。
- `Cargo.lock` 变化时检查 `cargo-sources.json` 已重新生成。

## 10. 里程碑

| 里程碑 | 范围 |
|---|---|
| M1 | 核心 Linux 后端与共用决策（本 PR）；`momentstays-gnome` 空壳 crate + Flatpak manifest 能构建运行，打开文件夹并扫描显示结果行 |
| M2 | 五步流程完整：方向、导入（portal + GridView）、特性开关（`action-blocking-reason`）、导出预览、执行与取消；zh-Hans/en |
| M3 | 预览：GStreamer paintable、封面/候选帧选择、手动封面；文件夹监听；zh-Hant；AppStream、图标、截图 |
| M4 | Flathub 提交与 CI 自动构建；跟随运行时升级（GNOME 51）验证；性能与内存预算调优 |

## 11. 风险

- **HEVC 只能来自 codecs-extra**：用户禁用扩展或离线安装时无 HEVC/AAC 解码。核心已降级处理：`image_decodes` 返回“未知”，取帧/重编码返回 `Unsupported`，音频校验记为“不可用”；界面需明确提示，而不是报“文件损坏”。
- **运行时约每 6 个月升级**（GNOME 50 → 51 …）：libheif/GStreamer/FFmpeg 版本变化可能改变解码细节；每次升级在 CI 跑 `linux-native` 测试与公开样片回归，旧运行时 EOL 前完成迁移。
- **与 Apple 后端的像素差异**：JPEG 质量映射、IDCT 舍入、ICC 字节、EXIF 改写方式不同（见 `core/LINUX_NATIVE.md`）；HDR 视频帧不做色调映射。输出可用但非逐字节一致。
- **Portal 文件访问**：只能访问用户选过的文件/文件夹；Documents portal 的 FUSE 路径上 `GFileMonitor` 可能收不到事件、`inode`/`ctime` 版本信息可能不稳定（影响“来源已变化”检测与复用），需实测并准备手动刷新。
- **内存预算**：`/proc/meminfo` 是宿主总内存，不反映 cgroup 限制。
- **域名/应用 ID**：若无法验证 `linofish.com`，需在首发前改用 `io.github.LinoFish.MomentStays`，之后不可再改。
