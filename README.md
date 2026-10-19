# Motion-Live

在 **Android 动态照片 JPEG / HEIC** 和 **Apple Live Photo 配对文件** 之间批量转换。为 **macOS 12 Monterey** 编写，提供 Intel / Apple Silicon 通用图形应用。

## 直接使用图形版

1.0.6 修复小米动态照片的 AAC 缓冲区重新分组误报：时间校验按每个实际样本执行，保留压缩数据、样本大小、原始 PTS 和封面位置。真实样本回归见 `python3 scripts/check_xiaomi.py`。

打开 [Motion-Live.app](dist/Motion-Live.app)，或把 [通用 ZIP](dist/Motion-Live-1.2.0-universal.zip) 发给其他人，解压即可使用。也提供拖入「应用程序」的 [DMG 安装包](dist/Motion-Live-1.2.0-universal.dmg)。

**接收者无需安装 Python、Swift、Xcode、FFmpeg、ExifTool、Homebrew 或其他运行环境。**图形版的解析、转换和界面均由原生 Swift 实现，只链接 macOS 自带框架。

1. 拖入或选择原始照片文件夹，应用自动扫描子文件夹。
2. 选择原片目录之外的保存位置，点击「开始转换」。
3. 在列表中查看进度与原因；可以停止、续转，在应用内查看报告并打开输出。
4. 将输出 JPG 或 HEIC 与 MOV 配对导入 Apple「照片」。

当前使用本地 ad-hoc 签名，**没有 Developer ID 签名和 Apple 公证**。网络分享后的首次启动可能被 Gatekeeper 拦截。macOS 12 可以在访达中按住 Control 点按应用 →「打开」；新版系统可在「隐私与安全性」中允许打开。完整步骤和限制见 [使用说明](packaging/使用说明.txt)。

Intel / macOS 12.7.6 已实机运行和界面检查；Apple Silicon 已交叉编译，尚未在 ARM 实机上验证。

## 命令行版本（可选）

只有运行源码命令行版时才需要 Python 3.9+ 和 Xcode Command Line Tools（`xcode-select --install`）。在本项目目录运行：

```bash
# 只读扫描
python3 -m motionlive scan "/Volumes/照片/Google 原片"

# 默认递归处理所有子文件夹，输出目录必须在输入目录之外
python3 -m motionlive convert "/Volumes/照片/Google 原片" -o "/Volumes/照片/Live Photo 输出"
```

也可以使用可执行入口 `./motion-live`。首次转换自动编译本机原生组件到 `.build/`；第一次编译 Swift 系统模块可能需要数分钟，以后使用缓存。

每张照片输出到独立文件夹，保留输入目录结构。下例为可选 Python CLI 的旧命名；原生 GUI 1.0.4 保留照片原始文件名：

```text
Live Photo 输出/
  Camera/
    sample-photo.jpg.livephoto/
      photo.jpg
      photo.mov
      manifest.json
  report-日期-随机编号.json
```

`.livephoto` 在这里是普通目录，并非 Apple 官方归档格式。把其中的 **JPG 或 HEIC 与 MOV 同时导入 Mac「照片」**，或在「照片 → 文件 → 导入」中选择输出目录。之后通过 iCloud 照片同步至 iPhone。建议先在「照片」中确认配对，再从「照片」分享至手机。

## 图像与 HDR 策略

| 输入 | 默认行为 |
|---|---|
| 普通 SDR JPEG，包括没有额外 HDR 数据的旧 Pixel HDR+ 成片 | 保持 SDR，不生成 Gain Map，不做 AI 提亮或反向色调映射 |
| 已有 Ultra HDR JPEG | 不转换 HDR 格式，不解码或重新编码主图/增益图；只修改 Live Photo 所需元数据与文件偏移 |
| 包含旧 Apple Gain Map 的 JPEG，且增益图能解码、版本和 MakerApple 33/48 完整 | 尝试按 Apple 公布的 Rec.709/亮度公式转为 Ultra HDR；**只重新编码增益图，主图 JPEG 压缩数据不变** |
| ISO 21496-1 JPEG 单增益图，具有可解释的正向增益、基础图色彩空间和完整参数 | 补充 Ultra HDR XMP 并保留 ISO 元数据；主图和增益图压缩数据均不变 |
| 其他旧版 HDR、无明确含义的附加图片、缺失亮度参数的 Gain Map | 保留原始静态数据，报告原因，不猜测 Gain Map 的含义或亮度 |

普通 HDR+ 算法生成的 SDR 成片不一定包含可恢复的 HDR。旧版转换支持可验证的 Apple Gain Map，以及有完整参数的 ISO 21496-1 JPEG 增益图；不支持从单张 SDR 推测真实 HDR。

旧 Apple Gain Map 目前只解释版本 `65536`。已有 ICC 原样保留；缺少 ICC 时，仅在 EXIF 明确声明 sRGB 的情况下补入系统 sRGB 配置，不猜测其他色彩空间。版本或色彩信息无法安全解释时保留原始 HDR 数据并记录原因。

关闭旧版 HDR 尝试转换：

```bash
python3 -m motionlive convert "/原片" -o "/输出" --legacy-hdr preserve
```

本工具不要求 macOS 12 能显示 HDR。最终 HDR 效果取决于观看设备、系统、相册及 iCloud 是否保留相关数据；不会给已有 Ultra HDR 自动补 ISO 21496-1 元数据。

## 封面时间

- 按 XML 命名空间读取 `MotionPhotoPresentationTimestampUs`，兼容旧 `MicroVideoPresentationTimestampUs`，支持属性和元素形式。
- 使用 AVAssetReader 的视频 presentation timestamp，不以文件日期、帧号或解码时间 DTS 代替。
- 同时检查样本时间与 edit list 的映射；若两种解释对应不同帧则拒绝猜测。可用 `--time-domain media` 或 `--time-domain presentation` 明确指定。报告保留原始时间、映射结果和输出位置，因此封面始终相对实际输出帧定位。
- 选择与源时间戳最近的实际视频样本；默认只允许半帧以内的差异，越界时报错。
- 在对应位置写入 `com.apple.quicktime.still-image-time` 定时元数据轨，持续时间来自实际相邻帧，而不是假定全部为 30fps。
- 写出后重新读取 MOV，验证封面定时轨、UUID、所有音视频压缩数据的 SHA-256、样本数量和 PTS。关键验证失败时不发布输出配对。
- 原始请求时间、输出封面时间、帧对齐误差和时间轴校验保存在 `manifest.json`。

静态照片可能由多帧融合生成，因此不保证它与视频中的某帧逐像素相等。这里保留拍摄文件声明的时间关系，不会拿视频截图替代原始高分辨率封面。

原片没有有效时间戳时默认失败并写入报告。只有明确接受推测位置时才使用：

```bash
python3 -m motionlive convert "/原片" -o "/输出" --missing-time midpoint
```

该选项只处理时间戳缺失；已有但越界的时间戳依然报错。

## 校验、续转和原片保护

```bash
python3 -m motionlive verify "/输出/Camera/某张.jpg.livephoto"
```

`verify` 对照转换时的 SHA-256 检查输出是否变化，并显示已完成的时间/HDR 校验记录；不是一次新的 Apple 图库导入测试。

重复转换会跳过原片、工具实现、参数、输出校验都一致的结果。不会覆盖已有的不同结果。输出先在隐藏暂存目录完成，校验成功后原子发布。原始照片只读，不调用 PhotoKit，不写入照片图库。

普通静态 JPEG/HEIC 跳过；损坏的动态照片与失败项单独记录。全批次完成后，只要有失败项就返回非零退出码。中断遗留的 `.motionlive-*` 暂存目录可以手动删除。

JPEG 编码表、帧/扫描头和熵编码数据在转换前后进行校验。因添加 EXIF 配对标识、移除 Google 动态视频声明及修正 MPF 地址，完整文件不可能逐字节相同。原有非 Apple MakerNote 不再作为活动 MakerNote 暴露，但数据仍保留在 EXIF 块中；拍摄时间、GPS、方向、ICC 和缩略图保持原有引用。已有 Apple MakerNote 的字段会保留并补入配对标识。

## 当前范围

- 图形版输入：Google/小米兼容 Container/MicroVideo、华为/荣耀 LIVE_、三星 SEF、OPPO/vivo 单文件 `.jpg` / `.jpeg`。
- 除已验证的三星 mpvd/SEF HEIC 外，其他 HEIC/AVIF、vivo 旧双文件暂不完整支持；不处理、任意拆分图片/视频；未知私有 HDR 不承诺转换。可选 Python CLI 仍面向 Google XMP，新增厂商兼容请使用图形版。
- 不能安全解释的格式、偏移或时间轴会报告错误，不会降级后声称完全保真。
- 带负数媒体 PTS 的文件目前拒绝转换；涉及多个编辑片段的复杂时间轴需要实片验证。
- 当前验证基于本地生成媒体和结构测试；还需真实 Pixel 各代样本，并在最终 iPhone「照片」中确认封面、HDR 和同步行为。

## 开发与验证

```bash
python3 -m unittest discover -s tests -v
python3 scripts/check_native.py
python3 scripts/check_cli.py
python3 scripts/check_gui_engine.py
```

第二条会编译组件，生成小尺寸媒体，检查 SDR、Ultra HDR、旧 Apple Gain Map、非零起点、可变帧率、失败回滚、续转和损坏检测；不会打开照片图库。需要可用的 AVFoundation 编码器。

图形版构建：`scripts/build_app.sh`，再运行 `scripts/make_dmg.sh`。构建者需要 Xcode Command Line Tools；使用打包后的应用不需要这些工具。Swift GUI 引擎的检查程序可用 `scripts/build_gui_tests.sh` 编译。

## 参考资料

独立重写，未复制未授权的项目代码，没有捆绑第三方运行库。

- [Google Motion Photo 规范](https://developer.android.com/media/platform/motion-photo-format)：布局、XMP、封面 PTS。
- [Google Ultra HDR 规范](https://developer.android.com/media/platform/hdr-image-format)：Gain Map、MPF、XMP。
- [Apple HDR 公式](https://developer.apple.com/documentation/appkit/applying-apple-hdr-effect-to-your-photos)：Rec.709、33/48 headroom、增益计算。
- [LivePhotoTools](https://github.com/reviventura/LivePhotoTools)（MIT）：参考 JPEG 元数据修改和无重编码路线；本工具补充封面定时轨。
- [live_motion_photos_convert](https://github.com/AssassinJY/live_motion_photos_convert)（MIT）：参考新旧 Google 封面字段及批量流程。
- [MotionTrans](https://github.com/Huakira/MotionTrans)：仅研究行为，未复制代码；其仓库没有授予通用开源许可。

### 1.0.1 识别修复

支持 Adobe Extended XMP（多分片、乱序重组及 GUID 校验）。仅改写动态照片字段；无关 Extended XMP 分片保留原字节。修复携带扩展元数据的 Pixel 原片被误判为非动态照片的问题。扫描中元数据损坏会显示「需检查」，详细原因可在列表中查看。

复查：`python3 -m unittest discover -v`，`bash scripts/build_gui_tests.sh`，`python3 scripts/check_extended.py`。本地 `pictures/` 原片不会包含在应用或分发包中。

### 1.0.2 批量导入修复

「选择原片」支持 Command / Shift 多选，可一次拖入多个 JPEG 或文件夹，并支持访达同时打开多个文件。批处理合并并去重所选输入，不扫描未选择的同级照片；来自不同文件夹的同名照片保留相对目录。

### 1.0.3 双视频轨与封面元数据恢复

Google Motion Photo 可包含主运动视频与辅助影像轨。按 [Google 规范](https://developer.android.com/media/platform/motion-photo-format) 的轨道顺序选择主视频，保留音频，以原始 JPEG 作为封面；不再拒绝合法的双视频轨文件。清单记录主轨、辅助轨 ID 与选择依据。

主 XMP 缺失时，仅在 JPEG 后紧邻一个完整 MP4、其中 Google `application/motionphoto-image-meta` 样本包含有效原始封面时间时尝试恢复。解析块边界、样本表与描述符，不搜索任意字节来猜视频位置；显式 MotionPhoto=0 仍不转换。恢复来源记入清单和报告。此路径目前用于原生 GUI；可选 Python CLI 的输入识别仍依赖 Google XMP。

回归脚本：`scripts/check_video_metadata.py`；本地用户样本的全量输出核验：`scripts/check_corpus.py`。用户原片与转换结果均不随应用分发。

### 1.0.4 保留原始文件名

例如 `sample-photo.MP.jpg` 输出同名 JPEG 与 `sample-photo.MP.mov`。以内容标识配对，清单记录实际文件名，允许读取旧版 `photo.jpg` / `photo.mov` 清单；仅针对已核验的旧版输出提供原子文件名升级，不重新编码图片或视频。已有图库项目不会被工具修改。

辅助视频轨按 Google 规范用于替代静态照片的额外画面，通常为较高分辨率、较低帧率的影像。省略它会失去这部分额外画面与完整往返还原能力；原始 JPEG、增益图、主运动视频及音频保持。当前工具不向 Live Photo MOV 写入辅助视频轨，原片始终保留。

### 1.0.5 轨道选择及独立辅助视频

GUI 主界面提供视频轨（主运动轨 / 辅助影像轨）与导出模式（仅 Live Photo / Live Photo ＋ 辅助轨 / 仅辅助轨）。单独辅助轨输出为原名称主体加 `.auxiliary.mov`，无音频、不编码、不改变样本 PTS。原分辨率与一帧等稀疏布局保留。

辅助轨用作 Live Photo 时，至少需要两帧，并通过原始封面时间映射与输出回读；缺轨、单帧或不能对应封面时明确报错，不用主轨、中点或循环图像替代。只有分辨率高，并不意味着能提供连续动态视频。

同时导出时请仅将同名 JPEG / MOV 配对导入「照片」，另外的 `.auxiliary.mov` 是独立视频；仅辅助轨模式输出在普通 `.auxiliary-export` 文件夹中。所有源文件保持只读，HDR 策略和文件名保留行为延续 1.0.4。

原生回归：`python3 scripts/check_auxiliary.py`（使用本地样本，以及由原始运动轨构造的双轨容器，验证真实连续辅助轨的选择与封面时间）。

高级设置另提供默认关闭的零时长辅助轨修复。只在独立导出时，对单编码样本、零时长、无复杂编辑或合成偏移的布局，用主轨最短正帧长修复 stts/mdhd/tkhd 的固定长度时长字段，mdat 和所有样本字节不改动。修复后的样本哈希还会与原 MP4 中该样本的直接哈希交叉核对。PTS 保持，显示时长发生有记录的变化；不生成额外帧，原片不变。

### 1.1.0 厂商容器、HDR 与静态载荷

支持华为/荣耀固定尾标及原始封面帧号、三星 SEF 目录、OPPO 的纯视频长度和 vivo V2 字段。输出 JPEG 仅包含经验证的静态 JPEG 图像和 HDR 增益图，移除内嵌视频、音频、非图片填充及厂商动态尾标。配对 MOV 单独保存动态画面和音频。

ISO 增益图在完整参数可解释时补充 Ultra HDR 元数据，双方 JPEG 均不重编码；已有 Ultra HDR 原样保留。厂商旧 HDR 缺少公开数学模型或真机样本时不猜测转换，详情见 [兼容与清理报告](兼容与清理报告-1.1.0.md)。

### 1.1.1 已有输出处理

图形版遇到已有输出时询问跳过、覆盖或停止，支持「将此选择应用于本次所有已有输出」。跳过计入跳过而不是失败；覆盖在新结果通过完整校验后原子替换整个配对文件夹，失败保留旧结果。新增回归：`python3 scripts/check_existing_output.py`。

### 1.1.2 可选数据文件及厂商

主界面可关闭「导出校验清单和 JSON 报告」：输出仅含媒体文件，内部仍完成压缩数据、封面时间及输出校验。本次报告保留在内存，仍能在应用内查看；不清理历史 JSON 文件。没有清单时无法自动校验续转，但已有输出的覆盖/跳过询问仍可使用。

厂商列优先读取 EXIF Make/Model，缺失时仅使用可确认的厂商容器识别；Google 通用格式不会被误认为 Google 拍摄设备。悬停查看型号和依据。验证：`python3 scripts/check_optional_data_and_vendor.py`。

### 1.1.3 荣耀私有 HDR 与窗口高度

新增本地荣耀 EHDR 样片识别和静态扩展原样保留，修复清理动态视频时丢失私有数据的问题。暂未找到可验证的私有 HDR 解码模型，因此不会声称已转成 Ultra HDR；Apple 仍显示 SDR，结果中明确提示。标准 Ultra HDR 继续保留原始增益图。详情和资料见 [荣耀 HDR 调查](荣耀HDR调查-1.1.3.md)。这类经过验证的私有静态扩展是早期“只保留 JPEG 图像”清理规则的例外；动态视频、音频仍只保存在独立 MOV。

窗口最小高度降为 540，并允许整页滚动；缩小窗口后仍能滚动访问底部转换按钮。真实样片回归：`python3 scripts/check_honor_private_hdr.py`。

### 1.1.4 三星 HEIC

新增三星 `mpvd + SEF/mpv2` 单文件 HEIC 动态照片处理。输出保留原名 HEIC + MOV，原始 HEVC 图像项目、分块、缩略图、色彩和方向属性均不重编码；只修改 EXIF 配对及 XMP 动态字段，并从 HEIC 移除整个动态视频/音频区域。静态图通过系统 ImageIO 读回配对标识，运动视频继续执行逐样本编码与时间校验。未知 HEIC 项目、复杂元数据布局、越界或重叠会明确拒绝，不转码兜底。

本地 HEIC 样片已成功生成 HEIC + MOV 配对，原始封面时间准确映射到输出帧，视频/音频编码和 PTS 保持。HEIC 不新增或转换 HDR。此验证不代表所有 HEIC、AVIF 或所有厂商均支持，也不代表验证了手机照片图库的 HDR 渲染。

### 1.1.5 紧凑界面

缩减顶部介绍、路径卡片与统计区，列表行高由 36 降至 28；两行设置面板集中轨道、导出模式、旧 HDR 和 JSON 选项。底部状态、进度、次要按钮与主操作整合为单行，任务结束隐藏进度条，小窗口保留全部操作。修正转换回调更新状态列的位置；转换引擎继续使用 1.1.4，旧输出校验和各项转换设置不受 UI 版本更新影响。

设计参考 Apple [macOS 信息密度与可调整窗口](https://developer.apple.com/design/human-interface-guidelines/designing-for-macos)、[工具栏操作分组](https://developer.apple.com/design/human-interface-guidelines/toolbars)、[进度指示器](https://developer.apple.com/design/human-interface-guidelines/progress-indicators)，以及 [Finder 状态栏](https://support.apple.com/en-gb/guide/mac-help/mchlp1774/mac) 和 [Xcode 活动区](https://developer.apple.com/documentation/xcode/configuring-the-xcode-project-window) 的紧凑状态组织方式。采用的是这些信息层级和反馈原则，具体尺寸为本应用布局选择，并非声称 Apple 规定了这些数值。


### 1.2.0 双向转换与窄屏布局

顶部可切换「苹果实况 → 安卓」，选择 Google/小米/三星/华为/荣耀/OPPO/vivo V2。输入为完整 JPEG/HEIC + MOV 配对，或通过「照片图库」读取选中的完整实况资源。按内容标识匹配，按视频定时元数据恢复封面，音视频不重编码。照片图库入口只读，首次使用需要系统权限，可能下载 iCloud 原件。

JPEG 默认 Ultra HDR：已有 Ultra HDR 保留；可验证的旧 Apple HDR 增益图转换为通用增益图；SDR 不生成增益图。主图和新生成增益图质量分别可调，默认保留 JPEG 原图编码。HEIC 只封装、不重新编码，保留全部图像和辅助项；目前 Google/小米/三星开放此格式，Apple 私有 HDR 在安卓端的显示仍需真机验证。未知 HDR 无法可靠转换时明确报错，不静默转 SDR。

窗口可收窄到 390 像素：路径卡片纵排，队列合并显示文件名/厂商/HDR/封面/状态，导出设置默认收起。底部操作区固定，展开设置或滚动时仍可转换、停止和打开结果。小于 700 像素切换窄屏，宽屏保留多列表格。常用控件采用 44 点高度约束。此版本仍为 macOS AppKit 应用，转换逻辑独立于界面；后续 iOS 需要另建 UIKit/SwiftUI 界面、图库和文件交互。

测试范围、格式限制和资料来源见 [双向转换与移动布局](docs/双向转换与移动布局.md)。Android 厂商写出方案通过本地解析与编码数据校验，尚未在各品牌真机相册验证。Photos 图库权限、iCloud 下载与实际图库导入需要在用户图库中验证。
