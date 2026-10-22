<p align="center">
  <img src="docs/branding/github-header.svg" alt="Motion-Live：留住原来的瞬间。保留封面时间、原始音视频流、HDR 增益图，与系统图库互通。" width="100%">
</p>

<p align="center">
  <a href="README.md">English</a> · <strong>简体中文</strong>
</p>

<p align="center">
  <strong>macOS 12+ · 原生 SwiftUI · 本地处理 · 版本 1.3.0</strong>
</p>

在 Android 动态照片与 Apple Live Photo 之间转换，留住你选定的封面瞬间。复用原始音视频，保留支持的图像与 HDR 数据，并将转换后的实况照片直接送回系统图库。

[构建与运行](#构建与运行) · [与系统图库互通](#与系统图库互通) · [保留哪些信息](#保留哪些信息) · [项目文档](docs/README.md)

**开源协议：[GPL-3.0-only](LICENSE)** · Copyright © 2026 Lino

## 留住原来的瞬间

| | Motion-Live 保留的内容 |
| --- | --- |
| **选定的封面位置** | 读取源封面时间戳，映射实际视频帧，并写入 Live Photo 封面定时标记。 |
| **原始视频和音频** | 复用压缩样本，不重新编码音视频；核对样本数据、数量、大小和呈现时间戳。 |
| **图像与 HDR 细节** | 在支持直通的路径保留 JPEG/HEIC 图像编码；保留已有 Ultra HDR 增益图，按明确模型处理支持的 Apple/ISO 增益数据。 |
| **现有图库工作流** | 从「照片」读取完整原片或对应编辑版，创建新的 Live Photo，并加入选定相册。 |

转换会改写容器与配对元数据。HEIC 转 JPEG、主动调整 JPEG 质量或转换 HDR 增益图可能涉及图像或增益图编码；视频和音频继续保留原始压缩样本。

## 与系统图库互通

既可以选择文件和文件夹，也可以通过 PhotoKit 直接读取系统「照片」图库。

| 来源 | Motion-Live 处理 | 去向 |
| --- | --- | --- |
| 文件中的安卓动态照片，或图库中仍含完整动态数据的原件 | 分离静态图与视频，保留封面时间，校验配对 | **直接导入相册的新 Live Photo**，或 JPEG/HEIC + MOV 文件 |
| 图库中的 Apple Live Photo，或完整图片 + MOV 配对 | 读取对应资源与封面定时标记，保留原始音视频 | 文件夹中的 **Android 动态照片**，或系统共享菜单 |

图库工作流包括：

- **按相册与范围选择。** 检查真实资源中的动态数据，识别可转换的照片。
- **默认读取原片。** 可选择完整编辑版；静态图片和视频始终来自同一选定版本。
- **直接创建 Live Photo。** 在同一个资产创建请求中加入静态图和配对视频，再关联目标相册。
- **延续图库信息。** 在可信来源信息可用时，保留资源文件名、拍摄日期与位置。
- **自主控制 iCloud。** 下载原件与应用运行期间的增量监测需要明确启用。
- **可恢复的交付。** 持久任务记录支持去重、入册/共享重试，以及应用自身缓存的安全清理。

源文件和来源图库资产保持不变。只有选择入册目标后才修改图库。应用会检查新资产与完整配对资源；真实图库播放、HDR 显示、iCloud 和接收设备兼容性仍需端到端验收。

## 保留哪些信息

### 封面时间戳，对齐实际视频帧

封面时间戳定义的是**动态视频中被选作封面的那个瞬间**。Motion-Live 读取源定时元数据，并依据实际媒体时间线映射到对应视频帧。生成 Live Photo 时，写入 `com.apple.quicktime.still-image-time` 定时标记；反向转换时读取该标记，恢复安卓动态照片的封面位置。

默认对齐容差为半帧。时间缺失、有歧义或越界时明确报告，不静默改用视频中点。可选清单记录源时间、映射帧与对齐误差。

「照片」中的拍摄日期是另一类元数据，在来源信息可用时单独传递给新图库资产。

### 复用原始压缩流，逐样本校验

视频和音频以**原始压缩样本**进入目标容器。输出校验包括样本哈希、大小、数量和呈现时间戳 PTS。

在支持直通的路径中，静态照片继续保留原始 JPEG/HEIC 图像编码。始终读取完整资源，不用缩略图或视频截图替代原始静态照片。已有输出仅在新结果完成校验后才替换。

### 保留有明确含义的 HDR 数据

| 来源数据 | 处理方式 |
| --- | --- |
| 已有 Ultra HDR JPEG | 保留增益图及参数；默认 JPEG 保留路径同时保留主图原始编码。 |
| 支持的 Apple Gain Map | 按支持的 Apple 亮度模型转换为通用增益图；增益图转换可能涉及编码。 |
| 可解释的 ISO 增益数据 | 保留图像编码，仅在完整参数可解释时补充元数据。 |
| HEIC 直通保留 | 保留图像项目、辅助数据及支持的属性；安卓端能否显示 Apple 私有 HDR 取决于接收设备。 |
| SDR 或未知私有 HDR | SDR 保持 SDR；明确报告不支持的 HDR，不虚构增益图或声称已完成转换。 |

部分已验证的私有静态扩展可以原样保留，但保留其字节不保证另一台设备能渲染该私有 HDR 格式。

## 支持的工作流

| 方向 | 格式与目标 |
| --- | --- |
| 安卓 → 苹果 | Google 兼容 Motion Photo JPEG，支持的华为/荣耀、三星 SEF、OPPO、vivo V2 容器，以及已验证的三星 HEIC 布局 → JPEG/HEIC + MOV |
| 苹果 → 安卓，JPEG | Google/通用、小米、三星、华为/荣耀、OPPO、vivo V2 |
| 苹果 → 安卓，HEIC | Google、小米、三星；保留已有 HEIC 编码 |

支持批量处理、进度、停止、校验后续转、跳过/覆盖、可选 JSON 清单与报告、独立辅助轨导出。单帧辅助轨不会被延长成虚构的动态画面。

厂商支持范围是解析器可以验证的容器布局，不代表所有机型或固件。详见 [格式范围](docs/product/双向转换与移动布局.md)。

## 构建与运行

构建者需要 Xcode Command Line Tools。默认生成 Intel macOS 应用：

```bash
git clone https://github.com/wildgooseL/Motion-Live.git
cd Motion-Live
bash scripts/build_app.sh
open dist/Motion-Live.app
```

使用已构建的应用，无需 Python、FFmpeg、ExifTool、Homebrew 或额外运行时。其他架构需显式选择：

```bash
bash scripts/build_app.sh --arm64
bash scripts/build_app.sh --universal
```

需要安装包时，从已构建应用生成 DMG、ZIP 与校验和：

```bash
bash scripts/make_dmg.sh
```

构建产物保存在本地 `dist/`。当前运行验证基于 Intel/macOS 12，ARM 真机尚待验证。应用采用 ad-hoc 签名，没有 Developer ID 签名或 Apple 公证，网络下载后可能需要在系统安全设置中允许打开。

### 第一次转换

1. 选择方向，添加文件、文件夹或系统图库范围。
2. 选择格式和交付目标：文件夹、相册或系统共享。
3. 开始转换并查看队列/报告；需要时停止或重试交付。

手动导入「照片」时，同时选择对应 JPEG/HEIC 和 MOV。独立辅助轨 MOV 是普通视频。输出文件夹必须位于原片目录之外。

### 可选 Python CLI

CLI 提供独立的 Google Motion Photo 工作流，不包含原生应用的全部厂商、HEIC 和图库功能。需要 Python 3.9+ 与 Command Line Tools：

```bash
python3 -m motionlive scan "/原片路径"
python3 -m motionlive convert "/原片路径" -o "/输出路径"
```

也可使用 `./motion-live` 入口，首次运行会把原生组件编译到 `.build/`。

## 本地处理与隐私

转换和校验在本地完成。个人照片样片、转换结果、完整报告、构建产物和编辑器截图均排除在仓库之外。项目视觉素材由代码绘制，不使用个人媒体。

可选报告可能包含源路径、EXIF、日期与哈希，应保存在本地。分享源码前运行：

```bash
python3 scripts/check_publish.py
```

## 开发

```bash
python3 -m unittest discover -s tests -p 'test_*.py'
bash scripts/build_gui_tests.sh
python3 scripts/check_gui_engine.py
python3 scripts/check_reverse.py
python3 scripts/check_workspace.py
```

原生布局检查使用虚构数据，需要可用的 macOS 桌面环境：

```bash
bash scripts/build_app.sh --diagnostics
python3 scripts/check_swiftui_ui.py
```

| 范围 | 位置 |
| --- | --- |
| SwiftUI、任务状态、图库与共享 | `gui/` |
| 解析、HDR、时间、转换与校验 | `native/` |
| 可选 Python 工作流 | `motionlive/` |
| 构建与回归入口 | `scripts/`、`tests/` |
| 应用属性与可复现图标绘制 | `packaging/` |
| 项目图标与 README 视觉素材 | [品牌素材](docs/branding/README.md) |

进一步阅读：[实现说明](docs/development/SwiftUI实现说明.md)、[图库规范](docs/design/图库识别与自动入册规范.md)、[共享规范](docs/design/输出位置与系统共享规范.md)、[验证范围](docs/validation/图库与共享-1.3.0.md)、[本地样片与隐私](docs/development/本地样片与隐私.md)。

当前尚未提供 iOS 应用。复杂时间线、未知 HEIF 结构和不支持的私有 HDR 可能被明确拒绝。构造媒体回归不能替代真实图库入册/播放、iCloud、第三方共享、手机相册 HDR，以及 ARM/iOS 真机验证。

## 开源协议

项目原创源代码、文档与视觉素材采用 **GNU General Public License 第 3 版，仅该版本**。允许商业使用；分发受 GPL 覆盖的派生作品时，需要按 GPL 提供对应源码并满足再分发要求。

详见 [协议全文](LICENSE)、[版权与版本选择](COPYRIGHT) 和 [第三方及分发说明](THIRD_PARTY_NOTICES.md)。个人输入媒体，以及独立授权的系统或第三方组件，保留各自权利与条款。
