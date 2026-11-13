# Google 连拍实验支持

首版接入 Google 旧式多文件静态 JPEG 连拍 → Apple，复用现有连拍检查、整组转换、原片归档、独立照片导出及图库回查。尚未用真实 Pixel 整组原片验证；不表示所有 Pixel、GCam 或 Top Shot 格式都已兼容。

## 输入范围

- 原片 EXIF Make 必须为 `Google`（大小写不敏感）。
- 文件名严格匹配 `IMG_YYYYMMDD_HHMMSS_BURSTnnn[_COVER].jpg/jpeg`，日期时间需有效，帧号允许 `000`。保留原帧号，不按时间邻近猜测顺序或补帧。
- 分组限定同一来源目录、同一 EXIF Model 和同一文件名组前缀。目录内同前缀但不同机型/来源的混合文件会使整组范围校验失败，需要整理原片后重扫。
- 不要求旧式照片一定有 XMP；有 XMP 时必须可完整解析，包括 Extended XMP。组内 `GCamera:BurstID` 必须全部缺省，或全部存在且一致。
- 如出现 `GCreations:CameraBurstID`，要求该文件同时有相同的 `GCamera:BurstID`。它不作为独立分组协议。
- `BurstPrimary` 接受 `1/0`、`true/false`、`True/False`。显式否定文件名 COVER、未知值、重复字段冲突均拒绝。多个代表标记需用户明确选择代表帧。
- 仅有 `BurstID`、没有严格帧文件名的照片不进入连拍路线。Top Shot、Motion Photo、Apple 实况配对、人像/景深标记组合不按普通连拍转换。重命名照片、`PXL_…` 或 HEIC 连拍尚未接入。

Google `BurstID` 也见于人像变体，不能单独作为真正连拍的判定。字段定义参见 [ExifTool Google 源码](https://raw.githubusercontent.com/exiftool/exiftool/master/lib/Image/ExifTool/Google.pm)，人像原始字段观察参见 [android-platform 讨论](https://groups.google.com/g/android-platform/c/uy7tlbTVmR0)。文件名规则限定为本轮实验接受的格式，不作为覆盖全部 Google 相机版本的规范。

## 输出与历史兼容

新来源使用 `source_format=google-burst` 和 `implementation=google-burst-1`，不冒用荣耀清单版本。现有荣耀版本与组标识算法保留。Swift 的扫描事实、组计划和转换选项传递 Google 来源及 XMP 组证据，避免中途丢失后走错验证路线。

整组范围在转换前后重新核对；转换前重新读取实际证据并核对扫描计划、来源版本和 SHA-256。完整静态 JPEG 可写 Apple 连拍标记；单帧或显式独立照片输出保持原样。缺帧可以处理，但不声明已知原始总数。原片完整归档，原始 JPEG 图像编码保持；写出文件与 Photos 导入后实际成组分别校验。

## 验证

[Google Rust 回归](../../core/tests/google_burst.rs) 覆盖零起始帧、Make 与文件名约束、属性/元素及命名空间 XMP、Extended XMP 与损坏数据、组标识/代表帧冲突、人像/实况排除、缺帧、部分扫描拒绝、独立导出、归档、图像编码/XMP 保留、续转及清单来源版本边界。既有荣耀、扫描与导出回归同时检查。

[Swift 工作区测试](../../apple/macos/tests/WorkspaceModelTests.swift) 的 `--google-burst-transport` 入口验证扫描事实 → Swift → Rust 组计划 → Swift 转换选项的格式、零起始序号、XMP 证据和确认签名；完整工作区测试也包含这些检查。

仍需真实设备验收：原始整组、连续拍摄的相邻两组、删除中间帧、修改代表帧及图库导出副本；确认实际文件名、字段取值、原图/HDR 保留，以及 Apple Photos 全成员同组与代表图回查。样片和包含位置/拍摄信息的报告按[本地样片与隐私](本地样片与隐私.md)仅保存在本地。
