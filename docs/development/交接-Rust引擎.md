# 交接：用 Rust 引擎替换 Swift 引擎

供在本地电脑继续这项工作的人或 agent 阅读。所有代码都在 git 里，云端没有未提交的改动。

## 目标（用户的要求）

1. 用 rust-core 的 core 替换 main 的旧 `core/`：旧 core 直接删除，缺的代码补齐。
2. Apple HDR 的读取用 Rust 实现。
3. 现有 UI 全部接入新内核，完全替代旧的 Swift 引擎。
4. 保留兼容性：Swift 版本转出的结果必须能被识别、续转、交付。
5. Swift 和 Rust 的对照测试由用户在本地 Mac 上跑。

固定约束：
- 不带 Android/JNI/cdylib。
- MP4 box 默认严格解析。
- 不带 Google 专用的 `momentstays` 命令行。
- 不动 `origin/core-replacement` 分支。
- 不合并分支历史。
- 不 force-push。

## 分支和 PR

| 分支 | PR | 内容 | 状态 |
|---|---|---|---|
| `core-merge` | #11 → `main` | 内核替换、ms_core 接口（op 0–3）、严格 box、恢复、样本上限、华为 `_c`、HEIC 封面冲突、Apple HDR 读取、Swift 接线；依赖与 main 的 `Cargo.lock` 一致 | CI 三个平台通过，待审 |
| `rust-engine` | #12 → `core-merge`（草稿） | `core/src/engine/` Rust 批处理引擎、`momentstays-driver`、`ms_core_engine` C 接口、内容 ID 改为 QuickTime keys 写法 | CI 三个平台通过 |

#12 叠在 #11 上：#11 合并后，把 #12 的 base 改成 `main`。

## 必读

- `core/ENGINE.md`：Swift → Rust 对照表、宿主回调、C 接口、与 Swift 已知的差异、本地对照方法。
- `core/README.md`、`core/ROADMAP.md`：内核概况和历史记录。ROADMAP 里 Android/JNI 的条目已不适用。

## 剩余工作（按顺序）

1. **本地对照**：先运行 `apple/macos/scripts/build_gui_tests.sh`，再运行 `apple/macos/scripts/run_checks_with_rust.sh`。
   - 对每个“Swift 通过、Rust 失败”的脚本，修 Rust，让输出和 Swift 一致。
   - 带“需要原生处理”的失败是预期的，第 2 步接上回调后才会通过。
2. **Swift 宿主回调**：在 `apple/shared/RustCore.swift` 里实现 `MsNative`。逐个回调对应的原 Swift 代码见 `ENGINE.md` 的“Host callbacks”表。
   - 回调会在多个 Rust 线程上并发调用，必须线程安全。
   - `existing_output` 可以阻塞等待用户选择。
3. **UI 接入**：
   - `NativeBatch.run/scan/convert`、`AndroidExport.pair/convert`、`AndroidTransfer`、`MotionPreview` 的封面计算、`PhotoLibraryService` 的导入检查，改为调用 `ms_core_engine`。
   - 请求模式见 `core/src/engine/request.rs`。
   - 沿用 `#if MOTIONLIVE_SWIFT_REFERENCE` 模式：参考构建保留 Swift 实现，用于对照。
   - iOS 的取消功能目前没有接，接入时一并接上 `ms_stop_cancel`。
4. **照片 App 验证**：Rust 写出的 MOV 导入照片 App 后，能否配成实况照片、封面帧是否正确。如果手头有 iPhone 原生 MOV，用 `exiftool` 对比元数据。
5. **删除旧 Swift 写入代码**：用户确认第 4 步通过后再删，包括 AVAssetWriter、MLJPEG/MLXMP 的写入部分。最后是否删除 Swift 对照模式由用户决定。

## 检查标准（每次提交前都必须通过）

```sh
cd core
cargo fmt --check
cargo clippy --release --all-targets -- -D warnings    # 当前 stable，另用 1.99.1 再跑一遍
cargo test --release --no-fail-fast
```

只有全部 0 告警、0 失败才提交。推送后等 GitHub Actions 三个平台都通过。

## 尚未验证

- 完整 Xcode/macOS/iOS App 构建。
- 照片 App 导入。
- 真实 iPhone HEIC 的 HDR 版本和 headroom 元数据（目前由 ImageIO 读取，见 `Native::heic_gain_map`）。
- iOS 上的 universal 构建。
