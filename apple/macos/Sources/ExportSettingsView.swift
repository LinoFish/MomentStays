import SwiftUI

struct ExportSettingsView: View {
    enum Section { case all, conversion, delivery }
    @ObservedObject var model: WorkspaceModel
    var actions = WorkspaceActions()
    var section: Section = .all

    private func binding<Value>(_ key: WritableKeyPath<ConversionSettings, Value>) -> Binding<Value> {
        Binding(get: { model.settings[keyPath: key] }, set: { value in model.updateSettings { $0[keyPath: key] = value } })
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                if section != .conversion {
                settingsGroup("输出位置") {
                    if model.settings.direction == .toApple {
                        Menu(AppText.text("输出：" + (model.settings.outputConfirmed == true ? model.settings.destination.title : "询问位置"))) {
                            Button(AppText.text("相册")) { actions.chooseDestination(.photos) }.disabled(model.settings.exportMode == .auxiliaryOnly)
                            Button(AppText.text("文件夹")) { actions.chooseDestination(.folder) }
                            Button(AppText.text("相册和文件夹")) { actions.chooseDestination(.folderAndPhotos) }.disabled(model.settings.exportMode == .auxiliaryOnly)
                        }.accessibilityIdentifier("output-destination")
                    } else {
                        Picker(AppText.text("输出方式"), selection: Binding(get:{ model.settings.destination },set:model.selectDeliveryMode)) {
                            ForEach(model.settings.destinations,id:\.self) { Text(AppText.text($0.title)).tag($0) }
                        }.accessibilityIdentifier("output-destination")
                    }
                    if model.settings.destination != .ask && !model.settings.destination.usesCache && model.settings.outputConfirmed == true {
                        HStack(spacing:8) {
                            Text(AppText.text("文件夹：") + (model.output?.lastPathComponent ?? AppText.text("尚未选择"))).lineLimit(1).truncationMode(.middle)
                            Spacer(minLength:0)
                            Button(AppText.text("选择文件夹…"),action:actions.chooseOutput).accessibilityIdentifier("choose-output-folder")
                        }
                    }
                    if model.settings.destination.importsPhotos {
                        note("导入生成并校验后的照片资源，不改原片。导入失败可在「最近结果」重试。")
                    } else if model.settings.destination.shares {
                        note("转换后打开系统共享菜单，可选 LocalSend 等扩展。取消或失败后，可在「最近结果」重试。")
                    }
                    UnconvertedExportControl(model:model,actions:actions,showsToggle:section == .all)
                }
                Divider()
                }
                if section != .delivery { compactActions }

            }
            .font(.body)
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .disabled(model.isBusy || model.sharing)
        }
        .accessibilityIdentifier("export-settings")
        .background(InterfaceStyle.canvas)
    }

    private var compactActions: some View {
        VStack(alignment:.leading,spacing:12) {
            if model.availableLive {
                Toggle(AppText.text("实况"),isOn:Binding(get:{ model.settings.liveConversion ?? true },set:{ value in model.updateSettings { $0.liveConversion = value } })).toggleStyle(.checkbox)
                if model.settings.liveConversion != false {
                    DisclosureGroup(AppText.text("格式、视频与封面选项")) {
                        if model.settings.direction == .toApple { appleOptions }
                        else if model.settings.direction == .toAndroid { androidOptions }
                        else { note(model.migrationTarget.detail) }
                    }.padding(.leading,16)
                }
                Divider()
            }
            if model.availableBurst {
                Toggle(AppText.text("连拍"),isOn:Binding(get:{ model.settings.burstConversion ?? true },set:{ value in model.updateSettings { $0.burstConversion = value } })).toggleStyle(.checkbox)
                if model.settings.burstConversion != false {
                    Button(AppText.text("选择连拍成员与代表照片…")) { model.burstReviewPresented = true }.padding(.leading,16)
                }
                Divider()
            }
            UnconvertedExportControl(model:model,actions:actions,showsLocation:false)
            if model.unconvertedExport != .none { note("完整输出未转换的原资源；位置在导出预览中选择。") }
            Divider()
            if model.availableTime {
            Toggle(AppText.text("照片时间"),isOn:Binding(get:{ model.settings.timeOptionsEnabled ?? (model.settings.photoTimeRepair == true || model.settings.exifFileTimes == true) },set:{ value in
                model.updateSettings { $0.timeOptionsEnabled = value; $0.photoTimeRepair = value && model.availableMetadataTime; if !value { $0.exifFileTimes = false; $0.photoTimeSynthetic = false } }
            })).toggleStyle(.checkbox)
            note("仅在相册中的照片时间错乱时开启。原片不改写。")
            if model.settings.timeOptionsEnabled ?? (model.settings.photoTimeRepair == true || model.settings.exifFileTimes == true) { timeOptions.padding(.leading,16) }
            }
            if model.availableHDR {
                Divider()
                Toggle(AppText.text("HDR（实验性，默认关闭）"),isOn:binding(\.legacyHDR)).toggleStyle(.checkbox)
                if model.settings.legacyHDR { note("仅尝试已支持的 HDR 配置；不支持项会在预览列出，不保证还原原厂显示效果。") }
            }
        }.font(.callout)
    }

    private var timeOptions: some View {
        settingsGroup("照片时间") {
            if model.availableMetadataTime {
            Toggle(AppText.text("补全缺失的拍摄时间（PhotoTime）"),isOn:Binding(
                get:{ model.settings.photoTimeRepair ?? false },
                set:{ value in model.updateSettings { $0.photoTimeRepair = value } }))
                .toggleStyle(.checkbox)
                .disabled(model.availableLive && model.settings.liveConversion != false && model.settings.direction == .toApple && model.settings.exportMode == .auxiliaryOnly)
                .accessibilityIdentifier("photo-time-repair-toggle")
            note("从原文件名识别日期或 Unix 时间，只补全输出副本中的缺失时间；已有非空时间保持不变。")
            }
            if model.settings.photoTimeRepair == true {
                Toggle(AppText.text("无法识别时生成合成排序时间"),isOn:Binding(
                    get:{ model.settings.photoTimeSynthetic ?? false },
                    set:{ value in model.updateSettings { $0.photoTimeSynthetic = value } }))
                    .toggleStyle(.checkbox).accessibilityIdentifier("photo-time-synthetic-toggle")
                note("合成时间不是真实拍摄时间；按原文件名自然顺序逐张增加一秒。时区：" + (model.settings.photoTimeZone ?? TimeZone.current.identifier))
                note("动态容器或 HDR 资源无法完整验证时不发布，也不会拆成静态图；请在导出预览查看原因。")
            }
            Toggle(AppText.text("将 EXIF 时间写入输出文件时间"),isOn:Binding(
                get:{ model.settings.exifFileTimes ?? false },
                set:{ value in model.updateSettings { $0.exifFileTimes = value } }))
                .toggleStyle(.checkbox).accessibilityIdentifier("advanced-exif-file-times-toggle")
            note("独立控制创建和修改时间，默认关闭；补缺后读取实际 EXIF，无可用 EXIF 时沿用来源文件时间。")
        }
    }

    private var experimentalOptions: some View {
        VStack(alignment:.leading,spacing:8) {
            Button { model.experimentalExpanded.toggle() } label: {
                HStack(spacing:6) {
                    Image(systemName:model.experimentalExpanded ? "chevron.down" : "chevron.right")
                        .font(.caption).foregroundColor(.secondary).frame(width:12)
                    Text(AppText.text("实验性功能")).font(.headline)
                    Spacer(minLength:8)
                    if model.settings.privateHDRExperimentEnabled {
                        Text(AppText.text("已启用")).font(.caption).foregroundColor(.secondary)
                    }
                }.frame(maxWidth:.infinity,alignment:.leading).padding(.vertical,4).contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityLabel(AppText.text("实验性功能"))
                .accessibilityValue(AppText.text(model.experimentalExpanded ? "已展开" : "已收起"))
                .help(AppText.text("展开或收起实验性功能；不改变已保存的开关偏好"))
                .accessibilityIdentifier("experimental-options").recordLayout("experimental-options")
            if model.experimentalExpanded {
                VStack(alignment:.leading,spacing:8) {
                    Toggle(AppText.text("尝试转换私有 HDR（实验性）"),isOn:binding(\.legacyHDR)).toggleStyle(.checkbox)
                        .disabled(!model.settings.privateHDRExperimentAvailable)
                        .accessibilityIdentifier("experimental-private-hdr-toggle")
                        .recordLayout("experimental-private-hdr-toggle")
                    note("仅适用于已支持配置，效果有局限，不保证原厂显示效果。不能处理的 HDR 会忽略，不阻断其他已支持步骤；标准 HDR 按原规则处理。")
                    if !model.settings.privateHDRExperimentAvailable {
                        note("当前转换方向或输出内容不使用此功能；保留你的开关偏好。")
                    }
                }.padding(.leading,18)
                }
        }
    }

    private var appleOptions: some View {
        VStack(alignment: .leading, spacing: 10) {
            settingsGroup("输出内容") {
                choice("导出内容", selection: binding(\.exportMode))
                if model.settings.exportMode == .withCandidates {
                    choice("高清封面输出",selection:Binding(
                        get:{ model.settings.candidateDelivery ?? .staticPhotos },
                        set:{ value in model.updateSettings { $0.candidateDelivery = value } }))
                }
                choice("视频来源", selection: binding(\.track))
                    .disabled(model.settings.exportMode == .auxiliaryOnly || model.settings.exportMode == .withCandidates || !(model.settings.coverSelections ?? [:]).isEmpty)
                if !(model.settings.coverSelections ?? [:]).isEmpty {
                    note("已选择高清封面，固定使用动态视频。")
                }
                if model.settings.exportMode == .auxiliaryOnly {
                    note("仅输出无声辅助视频，不生成 Live Photo。")
                } else if model.settings.track == .secondary {
                    note("辅助视频可能只有一帧，无法替代动态视频。")
                } else if model.settings.exportMode == .withAuxiliary {
                    note("辅助视频另存为 .auxiliary.mov，不属于实况配对文件。")
                } else if model.settings.exportMode == .withCandidates {
                    if model.settings.candidateDelivery == .livePhotos {
                        note("每个高清封面各生成一张 Live Photo，占用更多空间。")
                    } else {
                        note("主照片保留实况，其余高清封面另存为静态照片。")
                    }
                }
                if model.settings.exportMode != .auxiliaryOnly {
                    note("点击照片旁的「选择封面…」更换高清封面。新封面为 JPEG，不保留原封面的 HDR。")
                }
            }

        }
    }

    private var androidOptions: some View {
        VStack(alignment: .leading, spacing: 10) {
            settingsGroup("目标格式") {
                Text(AppText.text(model.migrationTarget.title)).font(.callout)
                note("更换转换方向或目标厂商，请返回第一步「转换方向」。")
                if model.settings.vendor == .google || model.settings.vendor == .honor {
                    note("新款荣耀推荐选择 Google / 通用；荣耀格式仅作备用。")
                    note("新款荣耀已验证可识别留刻导出的 Google 格式，不代表能识别 Google 相机拍摄的原片。")
                }
                VStack(alignment: .leading, spacing: 6) {
                    Text(AppText.text("照片格式")).foregroundColor(.secondary)
                    Picker(AppText.text("照片格式"), selection: binding(\.container)) {
                        Text(AppText.text(PhotoContainer.jpeg.title)).tag(PhotoContainer.jpeg)
                        if model.settings.vendor.supportsHEIC { Text(AppText.text(PhotoContainer.heic.title)).tag(PhotoContainer.heic) }
                    }.labelsHidden().frame(maxWidth: .infinity)
                }
                if !model.settings.vendor.supportsHEIC { note("此厂商仅支持 JPEG。") }
            }
            Divider()
            settingsGroup("图像质量") {
                if model.settings.container == .jpeg {
                    Toggle(AppText.text("保留 JPEG 原图编码"), isOn: binding(\.preserveJPEG))
                    quality("主图质量", value: binding(\.imageQuality))
                    quality("增益图质量", value: binding(\.gainQuality))
                    note("保留 JPEG 时不重压原图。HEIC 转 JPEG 使用主图质量；增益图质量仅在重编码时生效。")
                    note("HDR 使用 Ultra HDR，SDR 保持不变。")
                } else {
                    Label(AppText.text("保留 HEIC 原始编码"), systemImage: "checkmark.shield")
                    note("无需调整质量。兼容性和 HDR 显示需在目标手机确认。")
                }
            }
        }
    }

    private func choice<Choice: SettingsChoice>(_ title: String, selection: Binding<Choice>) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(AppText.text(title)).foregroundColor(.secondary)
            Picker(AppText.text(title), selection: selection) {
                ForEach(Array(Choice.allCases), id: \.self) { Text(AppText.text($0.title)).tag($0) }
            }.labelsHidden().frame(maxWidth: .infinity)
        }
    }

    private func quality(_ title: String, value: Binding<Double>) -> some View {
        VStack(spacing: 4) {
            HStack {
                Text(AppText.text(title))
                Spacer()
                Text(AppText.text("\(Int((value.wrappedValue * 100).rounded()))%"))
                    .monospacedDigit().foregroundColor(.secondary)
            }
            Slider(value: value, in: 0.01...1) { Text(AppText.text(title)) }
                .labelsHidden().accessibilityValue(AppText.text("\(Int((value.wrappedValue * 100).rounded()))%"))
        }
    }

    private func settingsGroup<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(AppText.text(title),systemImage:sectionSymbol(title)).font(.headline)
            content()
        }.frame(maxWidth: .infinity, alignment: .leading)
    }

    private func sectionSymbol(_ title: String) -> String {
        switch title {
        case "输出位置": return "folder"
        case "输出内容": return "livephoto"
        case "HDR 兼容": return "sun.max"
        case "校验与报告": return "checkmark.shield"
        case "目标格式": return "iphone"
        case "图像质量": return "slider.horizontal.3"
        default: return "gearshape"
        }
    }

    private func note(_ text: String) -> some View {
        Text(AppText.text(text)).font(.callout).foregroundColor(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }
}

struct ExportSettingsSheet: View {
    @ObservedObject var model: WorkspaceModel
    let availableSize: CGSize
    var actions = WorkspaceActions()
    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment:.top) {
                VStack(alignment:.leading,spacing:4) {
                    Text(AppText.text("处理与导出设置")).font(.title3.weight(.semibold))
                    Text(AppText.text(model.settings.direction.title)).font(.callout).foregroundColor(.secondary)
                }
                Spacer()
                Button(AppText.text("完成")) { model.settingsPresented = false }
                    .keyboardShortcut(.defaultAction)
            }.padding(12)
            Divider()
            ExportSettingsView(model: model, actions: actions)
        }
        .frame(width: min(440, max(350, availableSize.width - 24)), height: min(620, max(350, availableSize.height - 36)))
        .background(InterfaceStyle.canvas)
        .tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
        .dismissOnEscape { model.settingsPresented = false }
    }
}
