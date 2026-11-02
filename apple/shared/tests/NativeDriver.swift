import Foundation

@main struct NativeDriver {
    static func main() {
        do {
            guard let req = try JSONSerialization.jsonObject(with:FileHandle.standardInput.readDataToEndOfFile()) as? [String:Any] else { throw Failure("Missing request") }
            try emit(req["mode"] as? String == "fixture" ? fixture(req) : RustCore.engine(req))
        } catch { try? emit(["error":error.localizedDescription]); exit(1) }
    }
}
