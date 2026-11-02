# Rust 引擎接入与维护

macOS 生产路由通过 `apple/shared/RustCore.swift` 调用 `ms_core_engine`。批处理、解析、厂商封装、封面时间、HDR判断和无损重封装由 Rust 承担；详见 `core/ENGINE.md`。

Swift 保留 UI、增量扫描缓存、AVFoundation 预览、Photos、系统共享、输入清单和薄桥接。Rust `platform::SystemNative` 直接调用 Apple 系统 API 完成图像/增益图编解码、HEIC 增益图读取与视频帧提取；其他平台的 codec 后端显式未支持，不代表其他平台 App 已实现。

1.6.1 删除了已替代的 Swift 解析/写入/批处理、重复 HDR 转换、旧 helper 入口与 reference 编译开关。历史 git 及本地隔离归档保留对照证据，不再构建双引擎。`TestFixtures.swift` 只生成真实系统编码测试输入，不随正式 App 编译。

## 验证入口

```sh
cargo fmt --check --manifest-path core/Cargo.toml
cargo clippy --offline --locked --manifest-path core/Cargo.toml --all-targets -- -D warnings
cargo test --offline --locked --manifest-path core/Cargo.toml
bash apple/macos/scripts/build_gui_tests.sh
bash apple/macos/scripts/run_checks_with_rust.sh
bash apple/macos/scripts/check_motion_preview.sh
bash apple/macos/scripts/check_input_scan.sh /path/to/google-motion.jpg
```

driver 除 fixture 外均走实际生产 Rust 引擎；Python testkit 独立读取输出。照片 App 配对及 ARM 实机仍须单独验证，不能将编译通过替代真机验收。旧 Huawei 40 字节 LIVE_ 尾标/封面字段语义与私有载荷支持延期，既有严格解析规则保持不变。
