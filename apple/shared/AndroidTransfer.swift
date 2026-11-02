import Foundation

// Direction projection only; admission and conversion are owned by Rust.
enum AndroidTransfer {
    static let revision = "google-to-honor-a-modern-1"
    static func scanFile(_ source:URL, relative:String, bytes:[UInt8]? = nil, parsed:Result<NativeMotionPhoto,Error>? = nil) -> ScanRow {
        return RustCore.scanFile(source,relative:relative,direction:"android-to-android")
    }
}
