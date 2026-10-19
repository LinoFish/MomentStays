# 三星 HEIC 转换验证（1.1.4）

本地 HEIC 样片已生成 HEIC + MOV 配对，原片不改写。样片文件名和文件哈希未记录在此文档中。

原文件为移动设备 HEIC 静态容器，含主图 HEVC 分块、缩略图、EXIF 和 XMP；独立视频区域内含动态 MP4 与厂商定位信息。旧版本仅扫描 JPG/JPEG，并只处理 JPEG 数据，所以未支持这种输入。

新实现独立解析 HEIF 的 iinf / iloc / idat 等容器结构，删除整个 mpvd 区域。静态图不转 JPEG，不调用图片编码器。图像项目及其分块、缩略图、色彩/方向属性、项目引用均保持原始字节；EXIF 更新 Apple 配对标识，XMP 清理动态字段，旧元数据范围清零以移除失效的动态声明。新元数据存入新的 mdat，更新相应 iloc 位置，不移动图像项目。

验证结果：

- 全部 50 个图像/网格/缩略图项目逐字节相同，iprp / iref / pitm 不变。
- 系统 ImageIO 实际解码原图和导出图，图像尺寸和方向一致，解码像素哈希一致；导出图配对标识可正确读回。
- 原始 XMP 封面时间映射到源视频实际样本，误差小于一帧。未使用中点或文件尾部任意数字猜测封面。
- MOV 的 77 个视频样本和 135 个音频样本保留压缩数据，PTS 最大偏移 0。
- HEIC 不再含 mpvd / moov、SEF 动态尾部或 MotionPhoto 声明，视频音频只在 MOV 中。
- 专项异常测试覆盖越界 iloc、错误 SEF 指针、超出视频范围的封面；均拒绝发布结果。普通静态 HEIC 跳过，小写扩展名和不导出 JSON 均通过。
- 32 项原生引擎、荣耀私有 HDR/Ultra HDR、多文件导入回归通过。

编码与色彩属性原样保留，未新增增益图或转换 HDR。本样片在 macOS 12 的 ImageIO 解码为 8 位；不能以此宣称支持所有 HDR HEIC。手机「照片」图库配对及 HDR 渲染、Apple Silicon 真机尚未实测。当前支持已验证的三星 mpvd/SEF 布局，未知 HEIC 项目、加密或复杂元数据位置会明确拒绝处理。

HEIF 容器研究参考：[libheif 官方项目](https://github.com/strukturag/libheif)。实现使用 Swift 与系统框架，不把 libheif 或外部环境作为运行依赖。

可复核的测试：`scripts/check_samsung_heic.py`；像素读回程序：`tests/InspectStill.swift`。
