# 品牌依据

使用仓库真实品牌资产：`docs/branding/icon-light.svg`，以 base64 嵌入各单文件原型。品牌语句「留住原来的瞬间」。依据 `docs/branding/README.md`。

品牌琥珀色 #F4B860 用于暗房操作，灰白与墨色延续现有图标；工作台青灰来自示意山湖的中性色，旅程绿色来自风景插画的植被。三版使用系统中文字体，避免外部字体与网络依赖。

没有使用手机厂商标志。风景为代码绘制插画，不是真实照片或生成的用户媒体。


## 应用内图标核验（2026-10-25）

用户要求应用内沿用 GitHub 最新、官网也在用的图标。已在线读取 main 分支的 icon.svg、icon-light.svg、icon-dark.svg 与 packaging/Icon.swift，逐字节比较均与本地一致。官网 https://linofish.github.io/MomentStays/ 的 favicon 直接指向 main 分支 docs/branding/icon.svg，主视觉使用同一套环形、琥珀圆心和深浅色板。

SwiftUI 顶部品牌图标改为读取应用包内 AppIcon / AppIconDark（由现有官方图标绘制器生成），随 colorScheme 切换；不再用 livephoto 系统符号充当品牌图标。预览占位统一使用同一个品牌图标；缩略图角标改为媒体类型文字。
