// Opt-in native integration. iOS builds must include XMLCompat and the native Swift sources.
#if MOTIONLIVE_NATIVE_ENGINE && (os(macOS) || os(iOS))
import Foundation

public final class NativeConversionAdapter: ConversionEngine {
    public init() {}
    public func inspect(input: URL, cancellation: InspectionCancellation,
                        completion: @escaping (Result<InspectionResult, ConversionError>) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async {
            let result: Result<InspectionResult, ConversionError>
            if cancellation.isCancelled {
                result = .failure(ConversionError(code: .cancelled, message: "Inspection cancelled."))
            } else if !input.isFileURL {
                result = .failure(ConversionError(code: .invalidRequest, message: "Select a local file."))
            } else {
                do {
                    let bytes = Array(try Data(contentsOf: input))
                    let row = NativeBatch.scanFile(input, relative: input.lastPathComponent, bytes: bytes)
                    let classification: InspectionClassification = row.isMotion ? .motion : (row.confirmedStatic ? .staticImage : .unknown)
                    result = .success(InspectionResult(classification: classification, detail: row.detail))
                } catch {
                    result = .failure(ConversionError(code: .failed, message: error.localizedDescription))
                }
            }
            DispatchQueue.main.async {
                if cancellation.isCancelled {
                    completion(.failure(ConversionError(code: .cancelled, message: "Inspection cancelled.")))
                } else { completion(result) }
            }
        }
    }
    public func convert(request: ConversionRequest, progress: @escaping (ConversionProgress) -> Void,
                        completion: @escaping (Result<ConversionResult, ConversionError>) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async {
            guard !request.inputs.isEmpty, request.inputs.allSatisfy({ $0.isFileURL }), request.outputDirectory.isFileURL else {
                DispatchQueue.main.async { completion(.failure(ConversionError(code: .invalidRequest, message: "Select local input files and an output directory."))) }
                return
            }
            do {
                var options = ConversionOptions()
                if request.targetVendor != .apple {
                    options.direction = "to-android"
                    options.android.vendor = request.targetVendor.rawValue
                }
                let outcome = try NativeBatch.run(sources: request.inputs, output: request.outputDirectory,
                                                  options: options, token: StopToken()) { event in
                    let value = ConversionProgress(completed: event.index, total: event.total, item: event.relative,
                                                   status: event.status, detail: event.detail)
                    DispatchQueue.main.async { progress(value) }
                }
                guard !outcome.cancelled, let rows = outcome.reportData["files"] as? [[String: Any]] else {
                    throw Failure("Native batch did not return a complete result")
                }
                let items = try rows.map { row -> ConversionItem in
                    guard let source = row["source"] as? String, let name = row["status"] as? String,
                          let status = ConversionItem.Status(rawValue: name) else {
                        throw Failure("Native batch returned an unrecognized item")
                    }
                    return ConversionItem(source: URL(fileURLWithPath: source),
                                          output: (row["output"] as? String).map { URL(fileURLWithPath: $0) },
                                          status: status, detail: row["detail"] as? String ?? row["error"] as? String ?? "")
                }
                let result = ConversionResult(items: items, report: outcome.report)
                DispatchQueue.main.async { completion(.success(result)) }
            } catch {
                let failure = ConversionError(code: .failed, message: error.localizedDescription)
                DispatchQueue.main.async { completion(.failure(failure)) }
            }
        }
    }
}
#endif
