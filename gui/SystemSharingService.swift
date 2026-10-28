import AppKit

// Share file URLs, never NSImage. Menus/services are chosen by the user/system.
final class SystemSharingService: NSObject, NSSharingServicePickerDelegate, NSSharingServiceDelegate {
    private var picker: NSSharingServicePicker?
    private var service: NSSharingService?
    private var completion: ((String, Bool) -> Void)?
    var isActive: Bool { picker != nil || service != nil }
    func present(_ urls: [URL], window: NSWindow, update: @escaping (String, Bool) -> Void) {
        guard !isActive, let view = window.contentView, !urls.isEmpty else { return }
        completion = update
        let picker = NSSharingServicePicker(items: urls); self.picker = picker; picker.delegate = self
        update("已打开系统共享菜单", true)
        picker.show(relativeTo: NSRect(x: max(0, view.bounds.width - 160), y: 24, width: 120, height: 24), of: view, preferredEdge: .maxY)
    }
    func sharingServicePicker(_ sharingServicePicker: NSSharingServicePicker, didChoose service: NSSharingService?) {
        picker = nil
        guard let service = service else { finish("已取消共享；文件保留") ; return }
        self.service = service; service.delegate = self
        completion?("已选择 \(service.title)，等待共享应用处理", true)
    }
    func sharingServicePicker(_ sharingServicePicker: NSSharingServicePicker, delegateFor sharingService: NSSharingService) -> NSSharingServiceDelegate? { self }
    func sharingService(_ sharingService: NSSharingService, didShareItems items: [Any]) { finish("已交给 \(sharingService.title)；请在接收设备确认") }
    func sharingService(_ sharingService: NSSharingService, didFailToShareItems items: [Any], error: Error) { finish("共享调用失败：\(error.localizedDescription)；文件保留，可重试") }
    private func finish(_ state: String) {
        service = nil; picker = nil
        let callback = completion; completion = nil; callback?(state, false)
    }
}
