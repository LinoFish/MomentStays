import SwiftUI

struct ReportView: View {
    let report: ConversionReport
    let actions: WorkspaceActions
    @State private var filter = "all"
    @State private var selection: Int?
    private var entries: [ReportEntry] { report.entries.filter { filter == "all" || $0.state.rawValue == filter } }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 10) {
                Text(report.cancelled ? "已停止的任务" : "转换报告").font(.title2).fontWeight(.semibold)
                Text(report.summary).font(.callout).foregroundColor(.secondary).fixedSize(horizontal: false, vertical: true)
                Picker("显示", selection: $filter) {
                    Text("全部").tag("all")
                    ForEach([PhotoState.converted, .resumed, .static, .skipped, .failed], id: \.self) { Text($0.title).tag($0.rawValue) }
                }.frame(maxWidth: 240)
            }.padding(16)
            Divider()
            List(selection: $selection) {
                ForEach(entries) { entry in
                    HStack(spacing: 12) {
                        Text(URL(fileURLWithPath: entry.source).lastPathComponent)
                            .lineLimit(1).truncationMode(.middle)
                        Spacer(minLength: 0)
                        PhotoStatusLabel(state: entry.state)
                    }.padding(.vertical, 4).tag(entry.id).help(entry.source)
                }
            }.listStyle(.plain)
            Divider()
            if let entry = report.entries.first(where: { $0.id == selection }) {
                ScrollView {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(entry.source).fontWeight(.medium)
                        Text(entry.detail.isEmpty ? entry.state.title : entry.detail)
                        HStack {
                            Button("复制详情") { actions.copy(entry.source + "\n" + entry.detail) }
                            if let output = entry.output { Button("显示结果") { actions.reveal(URL(fileURLWithPath: output)) } }
                        }
                    }.font(.callout).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }.frame(maxHeight: 140).padding(16)
            } else {
                Text(entries.isEmpty ? "此分类没有文件" : "选择文件查看完整原因和输出位置")
                    .font(.callout).foregroundColor(.secondary).padding(16)
            }
            if let url = report.url {
                Button("显示 JSON 报告") { actions.reveal(url) }.padding([.horizontal, .bottom], 16)
            }
        }
        .background(InterfaceStyle.canvas)
        .tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
        .onChange(of: filter) { _ in if !entries.contains(where: { $0.id == selection }) { selection = nil } }
    }
}
