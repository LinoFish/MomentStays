# Motion-Live

Android 动态照片 JPEG / HEIC 与 Apple Live Photo 配对文件的双向转换工具。当前版本 **1.3.0**，使用 SwiftUI 原生界面，最低支持 macOS 12 Monterey。

[项目文档](docs/README.md) · [格式与功能范围](docs/product/双向转换与移动布局.md) · [工程结构](docs/development/SwiftUI实现说明.md)

## 功能

- 安卓 → 苹果：从动态照片提取静态图片与配对 MOV，保留原文件名、封面位置、图像与音视频编码。
- 苹果 → 安卓：按图片和 MOV 的内容标识配对，支持 Google/通用、小米、三星、华为/荣耀、OPPO 和 vivo V2 目标。
- 已有 Ultra HDR 原样保留；可解释的 Apple / ISO 增益图按明确模型处理，普通 SDR 不生成 HDR。
- 批量扫描、进度、停止、已有结果跳过或覆盖、可选校验清单与报告。
- 系统图库支持相册范围、原片/编辑版选择、按需 iCloud 下载及运行期间增量监测。
- 安卓转苹果可导入系统相册；苹果转安卓可使用系统共享。最近结果支持重试交付与清理应用缓存。

当前是 macOS 应用；iPhone / iPad 的 HTML 预览不代表已经提供 iOS 应用。图库写入、iCloud、第三方共享和各品牌相册的最终 HDR/动态效果仍需真实环境验收。

## 构建与运行

构建者需要 Xcode Command Line Tools。默认只构建 Intel 应用：

```bash
git clone https://github.com/wildgooseL/Motion-Live.git
cd Motion-Live
bash scripts/build_app.sh
open dist/Motion-Live.app
```

使用打包后的应用无需 Python、Swift、Xcode、FFmpeg 或其他运行环境。Apple Silicon 和通用构建需显式选择：

```bash
bash scripts/build_app.sh --arm64
bash scripts/build_app.sh --universal
```

仅在需要安装包时运行以下命令，脚本按应用实际版本和架构生成 DMG、ZIP 与校验和：

```bash
bash scripts/make_dmg.sh
```

构建产物位于本地 `dist/`，不随源码提交。应用采用本地 ad-hoc 签名，没有 Developer ID 签名或 Apple 公证；网络分发后首次打开可能需要在系统中允许。当前运行验证基于 Intel / macOS 12；ARM 实机尚待验证。

## 使用

1. 选择转换方向，添加照片文件、文件夹，或从系统图库选择范围。
2. 在导出设置中选择输出格式、文件夹、相册或系统共享方式。
3. 点击开始转换，在队列与报告中查看结果；需要时停止或重试交付。
4. 手动导入 Apple「照片」时，同时选择同名 JPEG/HEIC 与 MOV 配对。独立辅助轨 MOV 单独保存。

输出路径必须在原片目录之外。源文件保持只读；系统图库写入只在用户选择入册后执行。iCloud 下载和运行期间监测需要明确启用，应用退出后停止监测。详细说明见 [使用说明](packaging/使用说明.txt)。

## 可选 Python CLI

CLI 是独立的 Google Motion Photo 工作流，不等同于图形版全部厂商/HEIC 和图库能力。需要 Python 3.9+ 与 Command Line Tools：

```bash
python3 -m motionlive scan "/原片"
python3 -m motionlive convert "/原片" -o "/输出"
```

也可使用 `./motion-live`。首次转换会把原生组件编译到本地 `.build/`。

## 源码结构

| 路径 | 用途 |
| --- | --- |
| `gui/` | SwiftUI 工作区、状态模型、图库与系统共享服务 |
| `native/` | 格式解析、HDR、封面时间、批量转换与输出校验 |
| `motionlive/` | 可选 Python CLI |
| `scripts/` | 构建、打包与回归入口 |
| `tests/` | 构造媒体、原生驱动和状态模型测试 |
| `packaging/` | 应用属性、图标与分发说明 |
| `docs/` | 当前功能、设计、实现与验证边界 |

## 验证

基础验证使用构造媒体，不访问真实图库或发送文件：

```bash
python3 -m unittest discover -s tests -p 'test_*.py'
bash scripts/build_gui_tests.sh
python3 scripts/check_gui_engine.py
python3 scripts/check_reverse.py
python3 scripts/check_workspace.py
```

原生界面检查使用虚构数据，需要可用的 macOS 桌面环境：

```bash
bash scripts/build_app.sh --diagnostics
python3 scripts/check_swiftui_ui.py
```

真实样片回归属于可选本地验证，样片、配对资源和报告不随源码分发。配置方式见 [本地样片与隐私](docs/development/本地样片与隐私.md)，当前验证范围见 [发行验证](docs/validation/图库与共享-1.3.0.md)。

## 隐私与兼容边界

仓库只发布源码、脚本和匿名化文档。`pictures/`、`converted-*/`、`auxiliary-exports*/`、`.build/`、`dist/`、`.nimbalyst/` 与 `.local-archive/` 均不上传。输出报告可能含原片路径、EXIF、日期及哈希，应仅保存在本地。

复杂时间轴、未知 HEIF 结构和不能解释的私有 HDR 会明确报告限制。已有输出的版本标识用于校验和升级，不表示仓库仍保留旧版应用。格式资料与限制见 [格式与功能范围](docs/product/双向转换与移动布局.md)。
