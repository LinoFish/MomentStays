import Foundation

/// The requested output format. Input format is detected by the implementation.
public enum ConversionVendor: String, CaseIterable {
    case apple, google, samsung, xiaomi, huawei, honor, oppo, vivo
}

public struct ConversionRequest {
    /// Readable local files or directories. Apple inputs for Android export must include their paired movie.
    public let inputs: [URL]
    /// Writable directory outside the input directories.
    public let outputDirectory: URL
    public let targetVendor: ConversionVendor
    public init(inputs: [URL], outputDirectory: URL, targetVendor: ConversionVendor) {
        self.inputs = inputs; self.outputDirectory = outputDirectory; self.targetVendor = targetVendor
    }
}

public struct ConversionProgress {
    /// Completed active items and their total; total can be zero or change between implementations.
    public let completed: Int
    public let total: Int
    public let item: String
    public let status: String
    public let detail: String
    public init(completed: Int, total: Int, item: String, status: String, detail: String) {
        self.completed = completed; self.total = total; self.item = item; self.status = status; self.detail = detail
    }
}

public struct ConversionItem {
    public enum Status: String { case converted, resumed, skipped, failed }
    public let source: URL
    /// Published output directory when available, not necessarily a single media file.
    public let output: URL?
    public let status: Status
    public let detail: String
    public init(source: URL, output: URL?, status: Status, detail: String) {
        self.source = source; self.output = output; self.status = status; self.detail = detail
    }
}

public struct ConversionResult {
    /// Individual failures are represented here even when the overall operation succeeds.
    public let items: [ConversionItem]
    public let report: URL?
    public init(items: [ConversionItem], report: URL?) { self.items = items; self.report = report }
}

public struct ConversionError: LocalizedError {
    public enum Code { case invalidRequest, unavailable, cancelled, failed }
    public let code: Code
    public let message: String
    public var errorDescription: String? { message }
    public init(code: Code, message: String) { self.code = code; self.message = message }
}

/// Cooperative, thread-safe cancellation for inspection. Create a new token for each operation.
public final class InspectionCancellation {
    private let lock = NSLock()
    private var cancelled = false
    public init() {}
    public func cancel() { lock.lock(); cancelled = true; lock.unlock() }
    public var isCancelled: Bool { lock.lock(); defer { lock.unlock() }; return cancelled }
}

public enum InspectionClassification { case motion, staticImage, unknown }

public struct InspectionResult {
    public let classification: InspectionClassification
    public let detail: String
    public init(classification: InspectionClassification, detail: String) {
        self.classification = classification; self.detail = detail
    }
}

/// Implementations return immediately, deliver progress and exactly one completion on the main
/// queue, with no progress after completion. Overall failure is distinct from item failures.
public protocol ConversionEngine {
    /// Inspect the original local file, not a thumbnail or transcoded representation.
    /// Return motion only on positive evidence; uncertain and unsupported inputs are unknown.
    /// A cancelled operation completes once with .cancelled; cancellation is cooperative.
    func inspect(input: URL, cancellation: InspectionCancellation,
                 completion: @escaping (Result<InspectionResult, ConversionError>) -> Void)
    func convert(request: ConversionRequest,
                 progress: @escaping (ConversionProgress) -> Void,
                 completion: @escaping (Result<ConversionResult, ConversionError>) -> Void)
}
