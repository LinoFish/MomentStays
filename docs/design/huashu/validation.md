# 初稿验证

## HTML 方向初稿

Nimbalyst Chromium 浏览器验证：三版均在 1440×900 和 375×812 打开，documentElement.scrollWidth 未超过 innerWidth。桌面截图已查看，保存在 design-demos/screenshots/（PNG 按仓库隐私规则忽略，不进入版本库）。

已验证：工作台反向切换为 Motion Photo 并将交付位置设为文件夹；模拟转换完成后按钮恢复可用；暗房选中第三张插画并更新大图；滑杆读数更新至 1.85 s；两步引导正确前进、返回，并带入选择的交付位置。总览的三个嵌入页面均加载成功。

Skill 的 Python design_lint check/compare 因未安装 Python Playwright 自动跳过；本次用 Nimbalyst 浏览器实测及截图人工检查替代，没有声称通过自动设计体检。

未做真实转换与图库写入。单张插画没有动态视频，时间线用于演示布局和控件。此处记录初轮 HTML 方案阶段；后续 SwiftUI 实现验证见下文。

## SwiftUI 原生实现验证（2026-10-25）

最终诊断应用已编译为 macOS 12 / Intel 目标，并完成本地签名校验。运行 `python3 scripts/check_swiftui_ui.py`：15 个场景全部通过，覆盖三栏、390 像素窄窗、深色反向、设置面板、进度、共享、图库交付、空队列、错误、报告及千张队列的来回缩放。主要操作始终可见，队列高度至少 150，缩放保留选择和设置，无列表横向溢出。

已查看最终 wide.png 与 dark-reverse.png：顶部使用带琥珀圆心的官方品牌图标，并分别使用浅色与深色版本。截图来自 `.build/gui/ui-checks/` 的虚构样例，不涉及个人照片。

构建环境限制：沙箱内原生窗口服务会使诊断进程终止，iconutil 会误报 Invalid Iconset；在获准的正常 macOS 环境完成窗口测试、图标转换和签名，未更改图标绘制逻辑来绕过环境限制。


## 最终图标与配色调整

顶栏、缩略图占位和空状态均使用同一个 MomentStaysBrandIcon，读取官方深浅色 ICNS，并强制保留图像原色。缩略图上的媒体类型角标仅显示文字。背景使用系统窗口/内容颜色，强调色使用系统 accentColor，不再使用青灰色品牌替代色。已查看最终系统配色的 wide.png，并重新运行全部 15 个原生布局场景，均通过。

最终系统配色版本已通过 `bash scripts/build_app.sh`，生成 Intel/macOS 12 目标的应用包，并通过 codesign 的 deep/strict 校验。仅更新本地应用包，未生成或发布 ZIP/DMG。
