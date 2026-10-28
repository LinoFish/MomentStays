# DMG 构建

先运行 `bash scripts/build_app.sh --universal`，然后安装仅用于打包的依赖：

```bash
python3 -m pip install --target .build/dmg-tools -r packaging/dmg-requirements.txt
bash scripts/make_dmg.sh
```

安装包沿用 留刻（MomentStays）现有浅色图标、灰白背景与琥珀色箭头。背景不绘制任何文字提示，窗口只显示两个原生图标及文件名：应用位于左侧，Applications 链接位于右侧。手册通过应用内帮助入口打开，许可文件保存在应用包的 Resources 目录中。

`DmgBackground.swift` 绘制高分辨率背景，`dmg_layout.py` 在临时挂载的 HFS+ 卷中写入 Finder 布局和背景引用。构建无需控制 Finder，不需要自动化权限。所有中间文件位于被忽略的 `.build/`，退出时卸载临时卷并清理临时目录。

背景、卷图标和布局仅加入 DMG，ZIP 仍包含可拖入「应用程序」的应用本体。第三方 Python 打包工具不随应用分发。应用已有的 ad-hoc 签名与本次 DMG 视觉布局无关，不代表 Developer ID 签名或 Apple 公证。
