import SwiftUI

struct ExportSettingsView: View {
    @ObservedObject var model: WorkspaceModel
    var actions = WorkspaceActions()

    private func binding<Value>(_ key: WritableKeyPath<ConversionSettings, Value>) -> Binding<Value> {
        Binding(get: { model.settings[keyPath: key] }, set: { value in model.updateSettings { $0[keyPath: key] = value } })
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                settingsGroup("输出位置") {
                    VStack(alignment: .leading, spacing: 6) {
                        Picker("输出方式", selection: binding(\.destination)) {
                            ForEach(model.settings.destinations, id: \.self) { Text($0.title).tag($0) }
                        }
                        .accessibilityIdentifier("output-destination")
                    }
                    if !model.settings.destination.usesCache {
                        HStack(spacing: 8) {
                            Text("文件夹：" + (model.output?.lastPathComponent ?? "添加照片后自动建议"))
                                .lineLimit(1).truncationMode(.middle)
                                .help(model.output?.path ?? "请选择原片文件夹之外的输出位置")
                            Spacer(minLength: 0)
                            Button("选择文件夹…", action: actions.chooseOutput)
                                .accessibilityIdentifier("choose-output-folder")
                        }
                    }
                    if model.settings.destination.importsPhotos {
                        note("创建新的 Live Photo，不改原片。导入失败可在「最近结果」重试。")
                    } else if model.settings.destination.shares {
                        note("转换后打开系统共享菜单，可选 LocalSend 等扩展。取消或失败后，可在「最近结果」重试。")
                    }
                }
                Divider()
                if model.settings.direction == .toApple { appleOptions } else { androidOptions }
                Divider()
                settingsGroup("校验与报告") {
                    Toggle("导出清单与 JSON 报告", isOn: binding(\.exportDataFiles))
                    note("用于校验和续转。关闭后仍可查看本次报告。")
                }
                Divider()
                DisclosureGroup(isExpanded: $model.advancedExpanded) {
                    VStack(alignment: .leading, spacing: 14) {
                        if model.settings.direction == .toApple {
                            choice("缺少封面时间时", selection: binding(\.missingTime))
                        }
                        choice("封面时间解释", selection: binding(\.timeDomain))
                        if model.settings.direction == .toApple {
                            Toggle("修复辅助视频的零时长", isOn: binding(\.repairAuxiliaryDuration))
                                .disabled(model.settings.exportMode == .livePhoto)
                            note("仅影响独立辅助视频。高清封面的解码修复仅使用临时副本。")
                        }
                        note("自动检查封面位置，无法确定时报告错误。仅在熟悉原片时间轴时修改。")
                    }.padding(.top, 10)
                } label: { Text("高级设置").font(.headline) }
            }
            .font(.body)
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .disabled(model.isBusy || model.sharing)
        }
        .accessibilityIdentifier("export-settings")
        .background(InterfaceStyle.canvas)
    }

    private var appleOptions: some View {
        VStack(alignment: .leading, spacing: 20) {
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
            Divider()
            settingsGroup("HDR 兼容") {
                Toggle("尝试兼容其他 HDR 格式", isOn: binding(\.legacyHDR))
                    .disabled(model.settings.exportMode == .auxiliaryOnly)
                note("仅转换参数明确的增益图。已有 Ultra HDR 和普通 SDR 保持不变。")
            }
        }
    }

    private var androidOptions: some View {
        VStack(alignment: .leading, spacing: 20) {
            settingsGroup("目标格式") {
                choice("目标厂商", selection: binding(\.vendor))
                VStack(alignment: .leading, spacing: 6) {
                    Text("照片格式").foregroundColor(.secondary)
                    Picker("照片格式", selection: binding(\.container)) {
                        Text(PhotoContainer.jpeg.title).tag(PhotoContainer.jpeg)
                        if model.settings.vendor.supportsHEIC { Text(PhotoContainer.heic.title).tag(PhotoContainer.heic) }
                    }.labelsHidden().frame(maxWidth: .infinity)
                }
                if !model.settings.vendor.supportsHEIC { note("此厂商仅支持 JPEG。") }
            }
            Divider()
            settingsGroup("图像质量") {
                if model.settings.container == .jpeg {
                    Toggle("保留 JPEG 原图编码", isOn: binding(\.preserveJPEG))
                    quality("主图质量", value: binding(\.imageQuality))
                    quality("增益图质量", value: binding(\.gainQuality))
                    note("保留 JPEG 时不重压原图。HEIC 转 JPEG 使用主图质量；增益图质量仅在重编码时生效。")
                    note("HDR 使用 Ultra HDR，SDR 保持不变。")
                } else {
                    Label("保留 HEIC 原始编码", systemImage: "checkmark.shield")
                    note("无需调整质量。兼容性和 HDR 显示需在目标手机确认。")
                }
            }
        }
    }

    private func choice<Choice: SettingsChoice>(_ title: String, selection: Binding<Choice>) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).foregroundColor(.secondary)
            Picker(title, selection: selection) {
                ForEach(Array(Choice.allCases), id: \.self) { Text($0.title).tag($0) }
            }.labelsHidden().frame(maxWidth: .infinity)
        }
    }

    private func quality(_ title: String, value: Binding<Double>) -> some View {
        VStack(spacing: 4) {
            HStack {
                Text(title)
                Spacer()
                Text("\(Int((value.wrappedValue * 100).rounded()))%")
                    .monospacedDigit().foregroundColor(.secondary)
            }
            Slider(value: value, in: 0.01...1) { Text(title) }
                .labelsHidden().accessibilityValue("\(Int((value.wrappedValue * 100).rounded()))%")
        }
    }

    private func settingsGroup<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(title,systemImage:sectionSymbol(title)).font(.headline)
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
        Text(text).font(.callout).foregroundColor(.secondary)
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
                    Text("导出设置").font(.title3.weight(.semibold))
                    Text(model.settings.direction.title).font(.callout).foregroundColor(.secondary)
                }
                Spacer()
                Button("完成") { model.settingsPresented = false }
                    .keyboardShortcut(.defaultAction)
            }.padding(16)
            Divider()
            ExportSettingsView(model: model, actions: actions)
        }
        .frame(width: min(440, max(350, availableSize.width - 24)), height: min(620, max(350, availableSize.height - 36)))
        .background(InterfaceStyle.canvas)
        .tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
        .dismissOnEscape { model.settingsPresented = false }
    }
}
