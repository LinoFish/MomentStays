import SwiftUI
import Observation

// UI-only flow for this round (design-swiftui-addendum §3): placeholder data, no conversion backend.

enum MSDirection: Hashable {
    case androidToLive, liveToAndroid
    var title: LocalizedStringKey { self == .androidToLive ? "安卓 → 实况照片" : "实况照片 → 安卓" }
    var subtitle: LocalizedStringKey { self == .androidToLive ? "将动态照片转换为实况照片" : "转换为对方手机的动态照片" }
    var symbol: String { self == .androidToLive ? "livephoto" : "iphone" }
    /// Home button titles (Camo, msg267). The one-line explanation names both platforms.
    var buttonTitle: LocalizedStringKey { self == .androidToLive ? "转成实况照片" : "转成动态照片" }
    var homeSymbol: String { self == .androidToLive ? "livephoto" : "play.rectangle" }
}

enum MSSource: Hashable { case photos, files, scan }

enum MSRoute: Hashable {
    case select(MSDirection, MSSource)
    case scan
    case brand
    case converting
    case results
}

enum MSItemStatus: Equatable {
    case waiting, converting, done, failed(String), skipped
}

struct MSItem: Identifiable, Equatable {
    let id: Int
    let name: String
    var status: MSItemStatus
}

struct MSBrand: Identifiable, Hashable {
    let id: String
    let name: LocalizedStringKey
    static func == (lhs: Self, rhs: Self) -> Bool { lhs.id == rhs.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
    static let all: [MSBrand] = [
        MSBrand(id: "google", name: "Google"), MSBrand(id: "xiaomi", name: "小米"), MSBrand(id: "samsung", name: "三星"),
        MSBrand(id: "huawei", name: "华为"), MSBrand(id: "honor", name: "荣耀"), MSBrand(id: "oppo", name: "OPPO"), MSBrand(id: "vivo", name: "vivo")
    ]
    static func named(_ id: String) -> MSBrand { all.first { $0.id == id } ?? all[0] }
}

@Observable @MainActor
final class MSFlow {
    var path: [MSRoute] = []
    var direction: MSDirection = .liveToAndroid
    var source: MSSource = .photos
    var brandID = "samsung"
    var candidateCount = 12
    var selected: Set<Int> = []
    var items: [MSItem] = []
    var stopping = false
    var runID = UUID()
    /// Share, save and permission buttons only explain this round; they never claim success.
    var notice: LocalizedStringKey?

    private var retrying = false
    private static let demoFailures: Set<Int> = [3, 7]

    func open(_ direction: MSDirection, _ source: MSSource) {
        self.direction = direction; self.source = source
        candidateCount = source == .scan ? 6 : (source == .files ? 8 : 12)
        selected = []
        path.append(source == .scan ? .scan : .select(direction, source))
    }

    func candidateName(_ index: Int) -> String {
        source == .photos ? String(localized: "示意照片 \(String(format: "%02d", index))") : "IMG_\(2040 + index).jpg"
    }

    func startConversion() {
        let chosen = selected.isEmpty ? Array(1...8) : selected.sorted()
        // The design asks for an eight-row conversion sample, so a small selection is padded for the demo.
        let ids = chosen.count >= 8 ? chosen : Array(1...8)
        items = ids.map { MSItem(id: $0, name: "IMG_\(2040 + $0).jpg", status: .waiting) }
        retrying = false; stopping = false; runID = UUID()
        path = [.converting]
    }

    func retryFailed() {
        for index in items.indices { if case .failed = items[index].status { items[index].status = .waiting } }
        retrying = true; stopping = false; runID = UUID()
        path = [.converting]
    }

    func finish() { path = []; items = []; selected = []; stopping = false }

    var processed: Int { items.filter { $0.status == .done || $0.isFailed }.count }
    var succeeded: Int { items.filter { $0.status == .done }.count }
    var failed: Int { items.filter(\.isFailed).count }
    var skipped: Int { items.filter { $0.status == .skipped }.count }

    /// Simulated progress so every button walks through the whole flow.
    func run() async {
        for index in items.indices where items[index].status == .waiting {
            if stopping || Task.isCancelled { break }
            items[index].status = .converting
            try? await Task.sleep(for: .milliseconds(900))
            if Task.isCancelled { return }
            let fails = !retrying && Self.demoFailures.contains(items[index].id)
            items[index].status = fails ? .failed(String(localized: "未找到配对视频")) : .done
        }
        if Task.isCancelled { return }
        for index in items.indices where items[index].status == .waiting { items[index].status = .skipped }
        stopping = false
        path = [.results]
    }
}

extension MSItem {
    var isFailed: Bool { if case .failed = status { return true }; return false }
}
