# SwiftUI 主界面实现

当前版本 1.2.0，最低 macOS 12，使用 Command Line Tools 构建。SwiftUI 工作区复用原生媒体引擎，平台服务负责图库读取、Live Photo 入册与系统共享。个人素材与生成报告保存在忽略提交的本地目录。

## 当前界面

- 照片选择集中在顶部，空队列仅保留引导；输出方式、文件夹与相册的配置按钮统一放在设置面板；最近结果与停止监测仅在底部提供上下文入口。系统菜单与列表右键菜单继续保留常规操作。
- [iPhone／iPad 效果预览](../design/swiftui-ios-preview.mockup.html)展示相同信息架构在触控设备上的可能外观；HTML 交互示意不代表已实现 iOS target。
- 文件夹／相册弹窗从当前设置面板发起时，以该面板为父窗口；避免主窗口已有 sheet 导致请求被忽略，或新 sheet 排队至设置关闭后才出现。权限回调同时检查当前窗口及任务状态。参考 [Apple NSWindow sheet 管理](https://developer.apple.com/documentation/appkit/nswindow/sheets)和 [beginSheet 的排队行为](https://developer.apple.com/documentation/appkit/nswindow/beginsheet(_:completionhandler:))；本次未运行原生弹窗验证。
- 顶部操作栏：来源选择（主操作直接打开文件选择器，附加菜单提供图库入口）、方向、导出设置、开始／停止。
- 来源与输出以短摘要显示，完整路径可通过提示或复制查看。重新选择来源沿用替换语义，没有新增追加导入。
- 队列始终占据剩余空间，使用同一个 SwiftUI `List`。640 点以下采用两行，其他宽度采用文件名、HDR、封面时间和状态列；缩放不替换列表实例。
- 960 点起可显示设置检查器；其余宽度打开设置 sheet。侧栏与 sheet 使用同一个模型；sheet 打开时不重复出现侧栏。
- 内容高度不足 560 点时压缩摘要。队列、设置、详情分别滚动，不再用整页滚动套住队列。
- 原生按钮、Picker、复选框、Slider、DisclosureGroup、ProgressView；语义背景和文字颜色，无 Liquid Glass。
- 结果报告可筛选成功／已有／跳过／失败，查看和复制详情；关闭 JSON 时仍可读取内存报告。
- 菜单快捷键：⌘O 选择、⌘R 重新扫描、⌘Return 开始、⌘. 停止、⌘, 导出设置。Return 不在整个工作区无条件启动转换。

## 代码边界

```mermaid
flowchart TD
    Views[SwiftUI 工作区、队列、设置、报告] --> Model[WorkspaceModel / ConversionSettings]
    Views --> Actions[WorkspaceActions]
    Model --> Service[ConversionServing / ConversionService]
    Service --> Engine[现有 NativeBatch 扫描与转换]
    Actions --> Platform[MacPlatformServices]
    Platform --> System[文件面板、图库读取、Finder、覆盖弹窗]
    Host[AppDelegate 窗口与退出生命周期] --> Model
    Host --> Platform
```

| 文件 | 职责 |
| --- | --- |
| `gui/ConversionSettings.swift` | 类型化设置、默认值、厂商和格式约束、引擎参数映射 |
| `gui/WorkspaceModel.swift` | 主线程可观察状态、任务快照、稳定行标识、过期回调过滤、报告转换 |
| `gui/ConversionService.swift` | 后台扫描／转换、主线程回调、可取消的覆盖决策等待 |
| `gui/WorkspaceView.swift` | 操作栏、摘要、工作区、状态栏，依可用宽高调整布局 |
| `gui/PhotoQueueView.swift` | 原生列表、响应式行、选中详情、拖放入口 |
| `gui/ExportSettingsView.swift` | 分组设置，侧栏与 sheet 共用绑定 |
| `gui/ReportView.swift` | 报告列表、筛选与可复制详情 |
| `gui/InterfaceSupport.swift` | 平台动作接口、系统色、布局规则、状态标签 |
| `gui/MacPlatformServices.swift` | macOS 原生弹窗、文件和图库选择、临时资源所有权、结果打开 |
| `gui/App.swift` | AppKit 应用宿主、原生菜单、窗口关闭／退出保护 |
| `gui/PhotosLibraryPicker.swift` | 按相册、方向和版本选择图库资源，下载和监测需启用 |
| `gui/PhotoLibraryService.swift` | 原始资源读取、实况入库、相册关联和增量观察 |
| `gui/DeliveryModels.swift` | 持久任务、校验摘要、去重、共享暂存和缓存保护 |
| `gui/SystemSharingService.swift` | 完整文件 URL 的系统共享与交付状态 |
| `gui/UISnapshotSupport.swift` | 仅诊断构建包含的虚构数据截图与连续缩放检查 |

同步媒体工作始终在后台执行。来源和参数在开始时形成快照，忙碌时禁止修改；每次操作持有独立 ID，旧任务回调不能覆盖新任务。停止先取消令牌，等待引擎清理，再允许新任务。覆盖决策等待可被令牌取消解除，不依赖弹窗一定返回。

1.0 图库默认读取未修改原片，可明确选择完整编辑版；不混用或自动回退不同版本的配对。读取临时资源由 Mac 平台服务持有，退出时清理。转换后的任务缓存、交付文件和去重记录持久保存，可在最近结果中查看、重试交付与清理，不随 View 或应用退出删除。

## 后续 iOS 适配

可复用 `ConversionSettings`、`WorkspaceModel`、服务协议以及大部分 SwiftUI 队列／设置／报告组件。共享模型和视图不导入 AppKit；背景色、Escape 行为采用条件编译。`WorkspaceActions` 由宿主注入选择文件、图库、打开结果、复制和拖放等操作。

iOS 仍需新建 target，提供平台文件导入／导出、security-scoped URL、图库权限、后台任务与取消策略。需要逐项核对底层媒体 API 可用性与打包，不能因 SwiftUI 视图可共享就认定整个引擎已通过 iOS 验证。Mac 的紧凑窗口布局是复用基础，iPhone 的导航、安全区、触控尺寸和 sheet 展示仍需单独适配。

## 构建与验证

```bash
# 默认仅 Intel .app，无 ZIP / DMG
bash scripts/build_app.sh

# 用户请求安装包时，生成匹配版本和架构的 DMG / ZIP
bash scripts/make_dmg.sh

# 只有明确需要其他架构时使用
bash scripts/build_app.sh --arm64
bash scripts/build_app.sh --universal

# 状态、取消、参数快照、真实双向转换、覆盖决策和原片保护
python3 scripts/check_workspace.py

# 原生窗口的虚构数据检查，不访问照片图库
bash scripts/build_app.sh --diagnostics
python3 scripts/check_swiftui_ui.py

# 引擎已有回归
bash scripts/build_gui_tests.sh
python3 scripts/check_gui_engine.py
```

诊断包写入 `.build/gui/preview/MomentStays.app`，正式包写入 `dist/MomentStays.app`。诊断截图与几何数据位于 `.build/gui/ui-checks/`；正式包不编入示例注入和截图入口。系统图标打包、媒体编码和窗口服务需要正常 macOS 桌面访问权限。

界面检查覆盖宽、中、窄、低高度、深色反向设置、设置 sheet、进度、空队列、千行队列和连续跨断点缩放。几何检查证明主要操作与列表处于可见范围，不能替代 VoiceOver、真实键盘操作、实际图库权限或 ARM/iOS 真机验收。
