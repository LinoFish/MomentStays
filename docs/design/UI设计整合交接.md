# UI 设计整合交接

日期：2026-11-03。接收方：发布负责人 731405f9-f326-4ba3-82c1-c35108624c26。

本轮 UI 实现和本地诊断验收完成，生产源码编辑已停止，交由发布负责人统一整合冻结。当前尚未发布 1.7；最终版本核对、Universal 构建、提交、推送和 Release 均由发布负责人执行。本 UI 会话没有执行这些发布操作。

## 源码与文件所有权

源码位于当前共享工作区；下列文件包含本轮 UI 改动。部分文件在本轮前已有未提交改动，因此不能将整份 HEAD 差异视为本轮 UI 补丁。

- [apple/macos/Sources/InterfaceSupport.swift](../../apple/macos/Sources/InterfaceSupport.swift)：减少动态效果策略与轻微动画修饰器；来源按钮已有照片时转次要样式。
- [apple/macos/Sources/PhotoQueueView.swift](../../apple/macos/Sources/PhotoQueueView.swift)：舒适卡片、单个选择标记、图片内封面按钮、隐藏可转换与 SDR、HDR 展开、选择摘要与列表菜单、详情/预览过渡、内联任务反馈。保留手动主轨入口、全局选择与 expectedVersion 调用。
- [apple/macos/Sources/PhotoTimelineView.swift](../../apple/macos/Sources/PhotoTimelineView.swift)：候选直接位于时间线并按邻近位置分组；恢复原片清理提示与悬停；静态封面交叉淡化；加载失败恢复。保留已有真实 PTS、source_version、SHA256 和主轨手动选帧语义。
- [apple/macos/Sources/MotionPreview.swift](../../apple/macos/Sources/MotionPreview.swift)：新增 CoverTimelineMarker / CoverTimelineGroup 布局与候选诊断；已有预览引擎改动混合在同文件中。
- [apple/macos/Sources/WorkspaceView.swift](../../apple/macos/Sources/WorkspaceView.swift)：顶部全局操作、开始/停止原位切换、独立禁用策略、照片分类栏下约 60pt 横向进度与结果/失败/监测反馈；撤除全局底栏。
- [apple/macos/Sources/WorkspaceModel.swift](../../apple/macos/Sources/WorkspaceModel.swift)：本轮仅新增 #if UI_DIAGNOSTICS configurePreviewTask；其他生产模型改动不归本轮 UI 声明。
- [apple/macos/Sources/UISnapshotSupport.swift](../../apple/macos/Sources/UISnapshotSupport.swift)：HDR、任务阶段、真实停止点击、连续交互、减少动态效果与布局指标的诊断场景。
- [apple/macos/tests/MotionPreviewTests.swift](../../apple/macos/tests/MotionPreviewTests.swift)：新增 6 项候选布局/未知时间/分组检查，合计 49 项。
- [apple/macos/scripts/check_cover_ui.py](../../apple/macos/scripts/check_cover_ui.py)：封面与卡片场景扩至 8 项，断言图片内按钮、尺寸与点击不改变选择。
- [apple/macos/scripts/check_hdr_badge_ui.py](../../apple/macos/scripts/check_hdr_badge_ui.py)：新增 7 个 HDR 展开、布局稳定与减少动态效果场景。
- [apple/macos/scripts/check_workspace_motion_ui.py](../../apple/macos/scripts/check_workspace_motion_ui.py)：新增 21 个操作栏、进度与连续交互场景。
- [apple/macos/scripts/check_swiftui_ui.py](../../apple/macos/scripts/check_swiftui_ui.py)：本轮仅调整来源按钮强调色断言；同文件其他既有修改保留。
- [docs/design/UI交互与进度优化待办.md](../../docs/design/UI交互与进度优化待办.md)：已确认设计、实施清单和验收结果。
- 本地 `ux-audit-photo-cover-2026-11-03.md`：初始检查、实施记录、证据和未测边界；审计报告与截图不随源码提交。

辅助证据位于仓库根目录 `ux-audit-photo-cover-2026-11-03-evidence/`，由合成数据截图、指标及日志组成。证据目录仅本地保留，不随 App 打包或上传。

已将共享文件交回发布负责人。旧 Huawei 主轨手动处理、跨标签全局多选、来源版本校验与真实帧安全逻辑保持在共享源码中；本轮未用独立旧副本覆盖它们。最终整合仍需运行对应生产回归。

## 可审阅的源码快照

当前工作区是整合来源。完整文件比较快照位于 `.build/gui/ui-handoff-20261103-final/sources/`，SHA-256 与逐文件范围位于同目录 `manifest.json`；日志副本位于 `logs/`。这些完整文件包含共享工作区已有改动，仅用于比较和恢复参考，不是可盲目覆盖后续变更的独立 UI 补丁。快照不包含 App 或安装包。

## 设计验收

- 卡片只保留一个选择对号；图片右下角提供封面操作；卡片取消常规“可转换”与 SDR 标识。最小宽度 180pt、网格间距 16pt、常规图高 136pt，兼顾留白和空间效率。
- HDR 默认简写，悬停或键盘聚焦后轻微展开；私有 HDR 保留区别提示。取消选择进入列表更多菜单，不再独占一行。
- 原片及高清候选直接位于时间线上，邻近候选分组；不再保留下方候选选单。恢复原片和失败状态提示同步更新。
- 主操作集中顶部右侧，开始/停止原位切换；照片分类栏下方提供横向真实进度，未知进度不伪造百分比。移除常驻全局底栏，保留文件详情与异常恢复、共享、监测反馈，不遮挡照片。
- 选择、分类、详情、预览、确认封面与任务状态采用轻微过渡并尊重减少动态效果；逐帧悬停即时响应，网格不做批量重排动画。

## 已测

- 21 个原生操作栏、进度和交互场景通过：含真实点击停止、未知进度、失败/停止/交付/共享/监测、完成提示自动收起、连续切换、减少动态效果、千张照片缩放。
- 16 个相关原生窗口场景通过；HDR 7 场景、卡片/封面 8 场景、悬停预览 3 场景通过；预览与时间线布局检查 49 项通过。
- 最终诊断构建后另复核 5 个重点任务状态通过。已有本地 x86_64 UI_DIAGNOSTICS 构建成功；该产物和临时构建脚本均非发布依据。
- 交接前 `git diff --check` 通过。日志名和 SHA-256 记录在快照 manifest 中。

## 未测与已知边界

- 旧全量 UI 脚本在 output-menu 的旧断言停止：期待默认 folder 和“更改文件夹…”，当前代码默认 ask，菜单为“相册 / 文件夹 / 相册及文件夹”。未改动目的地语义；不能宣称该全量脚本通过。
- 未执行最终 Universal/ARM、生产优化构建、签名、公证或 Release 测试。发布负责人负责这些步骤。
- 未用个人照片测试真实 Photos 权限、图库导入导出、生产共享和旧 Huawei 全流程。手动选帧失败恢复完成编译与代码检查，未做个人照片运行验证。
- 未完成 VoiceOver 和全键盘人工认证；减少动态效果通过诊断参数验证策略，没有修改宿主系统偏好。动画检查侧重状态、布局和连续交互，并非逐帧人工曲线认证。
- 网格缩略图同步已选高清封面仍为后续建议，未扩大本轮缓存与解码范围。
