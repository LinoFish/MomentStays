# 大控件原生预览 Demo

独立 SwiftUI + AppKit 演示，延续已选定的三栏方向，支持 macOS 12+、Intel 和 Apple Silicon。

运行 `bash docs/design/large-controls/build_demo.sh`，解压 `.build/gui/large-controls-demo/MomentStays-Large-Controls-Demo.zip` 后打开应用。构建使用现有 `dist/MomentStays.app` 内的正式品牌图标。

顶部可切换常规与大控件。大控件使用 AppKit 原生按钮和弹出菜单，主操作高 44 pt、添加照片高 40 pt、方向与交付设置高 36 pt；常规模式对应 24 pt。颜色跟随系统，封面标记使用琥珀色。

可以切换方向、设置交付选项、增加示意照片、拖动时间条和演示转换进度。画面是代码绘制的示意插画，所有转换及文件选择均为演示状态，不读取实际照片。

已验证通用二进制、代码签名、深浅色截图、常规与大控件的实际布局尺寸。命令行 `--snapshot /absolute/path.png` 可保存截图与控件尺寸 JSON；可叠加 `--regular` 或 `--dark`。
