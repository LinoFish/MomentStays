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
                            Text(model.output?.lastPathComponent ?? "选择原片后自动建议")
                                .lineLimit(1).truncationMode(.middle)
                                .help(model.output?.path ?? "请选择原片文件夹之外的输出位置")
                            Spacer(minLength: 0)
                            Button("选择文件夹…", action: actions.chooseOutput)
                                .disabled(model.sources.isEmpty)
                                .accessibilityIdentifier("choose-output-folder")
                        }
                    }
                    if model.settings.destination.importsPhotos {
                        HStack {
                            Text(model.settings.albumTitle).lineLimit(1)
                            Spacer()
                            Button("选择相册…", action: actions.chooseAlbum)
                                .accessibilityIdentifier("choose-output-album")
                        }
                        note("新增独立 Live Photo，不修改来源。成功后加入指定相册；失败可在最近结果中重试。")
                    } else if model.settings.destination.shares {
                        note("交付完整 JPG/HEIC 文件。转换结束后打开系统共享菜单，可选择 LocalSend 等已启用扩展。")
                        note("共享取消或失败时保留文件，可在最近结果中再次共享。")
                    }
                }
                Divider()
                if model.settings.direction == .toApple { appleOptions } else { androidOptions }
                Divider()
                settingsGroup("校验与报告") {
                    Toggle("导出清单与 JSON 报告", isOn: binding(\.exportDataFiles))
                    note("清单用于校验和续转。关闭后仅导出媒体，仍可查看本次报告。")
                }
                Divider()
                DisclosureGroup(isExpanded: $model.advancedExpanded) {
                    VStack(alignment: .leading, spacing: 14) {
                        if model.settings.direction == .toApple {
                            choice("封面时间未记录时", selection: binding(\.missingTime))
                        }
                        choice("封面时间的含义", selection: binding(\.timeDomain))
                        if model.settings.direction == .toApple {
                            Toggle("允许修复辅助轨的零时长", isOn: binding(\.repairAuxiliaryDuration))
                                .disabled(model.settings.exportMode == .livePhoto)
                            note("仅影响独立导出的辅助轨；修复会记录在清单中，默认关闭。")
                        }
                        note("自动模式检查封面位置，遇到歧义会报告错误。仅在了解原片时间轴时修改。")
                    }.padding(.top, 10)
                } label: { Text("高级参数").font(.headline) }
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
                choice("导出模式", selection: binding(\.exportMode))
                choice("视频轨道", selection: binding(\.track))
                    .disabled(model.settings.exportMode == .auxiliaryOnly)
                if model.settings.exportMode == .auxiliaryOnly {
                    note("仅导出独立辅助视频；不生成 Live Photo，不含音频。")
                } else if model.settings.track == .secondary {
                    note("辅助轨可能只有一帧，不能代替连续动态视频。")
                } else if model.settings.exportMode == .withAuxiliary {
                    note("辅助轨另存为 .auxiliary.mov，请与实况配对分开保存。")
                }
            }
            Divider()
            settingsGroup("HDR 兼容") {
                Toggle("尝试兼容非 Ultra HDR", isOn: binding(\.legacyHDR))
                    .disabled(model.settings.exportMode == .auxiliaryOnly)
                note("仅处理参数明确的增益图。已有 Ultra HDR 原样保留，普通 SDR 不生成 HDR。")
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
                if !model.settings.vendor.supportsHEIC { note("此厂商当前支持 JPEG 输出。") }
            }
            Divider()
            settingsGroup("图像质量") {
                if model.settings.container == .jpeg {
                    Toggle("保留 JPEG 原图编码", isOn: binding(\.preserveJPEG))
                    quality("主图质量", value: binding(\.imageQuality))
                    quality("增益图质量", value: binding(\.gainQuality))
                    note("保留编码时不重压 JPEG。HEIC 转 JPEG 仍使用主图质量；增益图质量仅用于需要重新编码的增益图。")
                    note("HDR 输出采用 Ultra HDR；普通 SDR 不生成增益图。")
                } else {
                    Label("保留 HEIC 原始编码", systemImage: "checkmark.shield")
                    note("无需调整压缩质量。厂商识别和 HDR 显示仍需在目标手机验证。")
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
            Text(title).font(.headline)
            content()
        }.frame(maxWidth: .infinity, alignment: .leading)
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
            HStack {
                Text("导出设置").font(.headline)
                Spacer()
                Button("完成") { model.settingsPresented = false }
                    .keyboardShortcut(.defaultAction)
            }.padding(16)
            Divider()
            ExportSettingsView(model: model, actions: actions)
        }
        .frame(width: min(440, max(350, availableSize.width - 24)), height: min(620, max(350, availableSize.height - 36)))
        .background(InterfaceStyle.canvas)
        .dismissOnEscape { model.settingsPresented = false }
    }
}
