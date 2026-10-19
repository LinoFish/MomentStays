# 荣耀私有 HDR 调查（1.1.3）

结论：两张样片都能识别为荣耀动态照片，封面为从 0 开始的第 36 帧。通用 Ultra HDR 可以原样保留。另一张具有荣耀私有 EHDR 静态扩展，目前不能可靠地转换为 Ultra HDR；本次修复识别和静态扩展保留，避免旧版在清理动态视频时丢掉私有数据。保留数据不等于 Apple 能渲染该私有 HDR，也未验证重新导回荣耀相册后的显示。

## 样片对比

- 私有 HDR 样片：无 MPF 和标准 HDR XMP。主 JPEG 与视频之间包含荣耀私有静态扩展，不是一个可直接取出的 JPEG 增益图。当前版本验证其记录结构后逐字节保留静态扩展；明确警告暂未转换为通用 HDR。未知版本或冲突长度拒绝处理，避免静默丢弃。
- 通用 Ultra HDR 样片：主 JPEG 与 MPF 增益 JPEG 同时带 Adobe HDRGM 和 ISO 21496-1 元数据。保留主图编码、完整增益 JPEG 和增益参数。
- 两张主图压缩数据不同，不能因为画面相同就把后一张的增益图直接套给前一张。这无法证明与私有格式的原始 HDR 渲染一致。
- 两张视频均使用容器提供的原始封面帧位置，并由原始视频样本计算封面时间。私有格式样片存在非零起始 PTS，不能用帧号除以帧率替代原始样本时间。

## 网络资料与可行路径

查找了上述私有签名、荣耀 EHDR / CDRDATA、私有 HDR 转换等关键词，没有找到可以验证上述私有块数学含义的公开解码器或格式规范。这个结论仅表示本次检索未找到，不能据此断言不存在实现。

[Google libultrahdr](https://github.com/google/libultrahdr) 支持将已知 SDR 主图、增益 JPEG 和明确增益参数组合为 Ultra HDR，也支持从真实 HDR 像素生成增益图。它不提供本样片荣耀 EHDR 私有块的解码实现。缺少私有 HDR 像素或可验证的变换参数时，不能凭该库恢复原始亮度效果。

[荣耀官方论坛的用户反馈](https://club.honor.com/cn/opinion_thread-list.html?filter=dateline&mobile=no&type=3) 提到在荣耀相册编辑后可产生通用 Ultra HDR。这是用户经验，不是公开转换 API 或协议保证。实际可用的路径是先在支持的荣耀相册中导出通用 HDR，再交给本工具配对；本次附带的通用格式样片可直接使用。相册是否重编码主图、不同机型版本是否支持，需要单独验证。

本工具不会对 SDR 凭空生成增益图，也不会把未知私有数据标记成已转换的 Ultra HDR。

## 窗口高度

原最小窗口高度 760，接近较小屏幕可用高度；固定内容的高度约束进一步限制缩小。现最小高度 540，整页可纵向滚动，较大的窗口继续扩展列表区域。窗口缩小后底部转换按钮可滚动到达。

## 验证

执行 `scripts/check_honor_private_hdr.py` 验证真实样片、异常长度/版本、旧结果保护、主图编码、增益图、静态扩展和封面时间（另用独立 Python stts/ctts 解析交叉核对）。原始样片不改写。窗口另用实际 AppKit 运行在不同高度截取布局，随快照记录实际窗口高度及滚动位置。iPhone 照片图库 HDR 渲染未做真机验证。

## 补充：沿华为格式继续排查

根据“荣耀可能与华为同格式”的建议，进一步读取了 [OpenHarmony 官方 hdr_helper.cpp](https://github.com/openharmony/multimedia_image_framework/blob/master/plugins/common/libs/image/libextplugin/src/hdr/hdr_helper.cpp) 及其 MPF 解析器，并下载官方公开测试图片作本地对比（没有上传用户照片）。

确认源码里存在以下 JPEG 路径：

1. **HDR Vivid 双层 JPEG**：`GetVividJpegGainMapOffset` 要求 APP8 的 `ITUT35` 标记和明确的增益图偏移。官方 `HdrVivid.jpg` 具有该标记、MPF 双图和 ISO 标记。荣耀私有样片的 APP8 是另一段数据，不以 ITUT35 开头，整张原片也没有该字符串，因此不能套用这条解析路径。
2. **旧 CUVA JPEG**：`GetCuvaJpegGainMapOffset` 将 APP5 交给动态加载的 `libimage_cuva_parser.z.so`，调用 `GetCuvaGainMapOffset`；这份公开源码并没有包含该解析函数本体。官方 `hdr.jpg` 有带 ITUT35 的 APP5，主 JPEG 后紧跟一个 JPEG 增益图。荣耀私有样片没有 APP5，在静态扩展中也没有 JPEG SOI，不能用该样本布局提取增益图。这只是本次检索到的依赖，不能断言该库或其他实现绝无公开来源。
3. **ISO 21496-1 双层 JPEG**：通过 APP2 ISO 标记和 MPF 定位增益图。荣耀的通用样片符合；私有样片两者均无。本工具已有 ISO 增益图转 Ultra HDR 元数据的路径，不需要把荣耀私有块伪装成 ISO。

另下载的 `hdr_1.jpg` 在这三个入口下均不成立，尾部为其他相机数据；不能单凭文件名 hdr 就把它当成已验证的 HDR 规范样本。

这说明：**目前检索到、可核验的华为 JPEG 路径不能直接解释本张荣耀 EHDR 数据，但不足以排除它与其他华为旧格式同源。** 动态照片的 LIVE_ 尾标共用，也不意味着 HDR 载荷和亮度变换完全相同。

[华为 ImageProcessing API](https://developer.huawei.com/consumer/cn/doc/harmonyos-references-V14/image__processing_8h-V14) 提供双层/单层 HDR 的 Compose/Decompose 接口，是在支持的系统上解码已支持图像后处理的另一条研究路径；它不是可直接打包进 macOS 12 的通用私有 EHDR 解码器，也没有证据证明能解码本张荣耀样片。未调用真机接口，不声称已验证兼容。

公开样本与代码记录保存在本地研究缓存，格式对比包括源文件 SHA256 和标记位置；本次只新增私有静态扩展保护，没有添加推测的亮度算法。

实际转换尝试：将官方 `HdrVivid.jpg` 与本地现成运动视频组合为测试动态照片，仅写入明确的测试 XMP 时间，再运行本工具。该样本的 ISO 参数标志不符合本工具可无损补充 Adobe 元数据的条件，因此结果明确报告未完成 Ultra HDR 转换；主图编码及增益 JPEG 均原样保留。这不算华为 HDR 转换成功，也不代表验证了原厂 Vivid 渲染兼容。没有把该测试结果冒充荣耀私有样片转换成功。

最终验证：荣耀专项检查全部通过；32 项原生引擎检查通过；厂商/ISO HDR、可选 JSON、输出命名回归通过。已核对 pictures 内本次记录的 142 个文件 SHA256 不变。最终 ZIP 解压后的 AppKit 程序实际运行成功，窗口高度 540 时文档高度 647、可滚动 135，底部按钮可见；放大受本机屏幕限制到 705 时无需整页滚动。通用二进制签名和 DMG 校验通过；Apple Silicon 运行和手机 HDR 渲染未实测。
