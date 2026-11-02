import Foundation

/// A caller-assigned identifier distinguishes even requests with identical inputs.
public struct BatchConversionRequest {
    public let id: UUID
    public let request: ConversionRequest
    public init(id: UUID = UUID(), request: ConversionRequest) {
        self.id = id; self.request = request
    }
}

public enum BatchConversionOutcome {
    /// Inspect result.items as well: a successful engine operation may contain failed items.
    case completed(ConversionResult)
    case failed(ConversionError)
    /// The request was not started because cancellation was observed.
    case cancelled
}

public struct BatchConversionItemResult {
    public let id: UUID
    public let outcome: BatchConversionOutcome
}

public struct BatchConversionProgress {
    /// Number of requests with an outcome, including requests cancelled before starting.
    public let completed: Int
    public let total: Int
    /// The current engine's advisory progress, if this update came from that engine.
    public let activeID: UUID?
    public let activeProgress: ConversionProgress?
}

/// Thread-safe scheduling cancellation; does not interrupt an in-flight engine conversion.
public final class BatchConversionCancellation {
    private let lock = NSLock()
    private var cancelled = false
    public init() {}
    public func cancel() { lock.lock(); cancelled = true; lock.unlock() }
    public var isCancelled: Bool { lock.lock(); defer { lock.unlock() }; return cancelled }
}

/// Runs requests serially. All callbacks are asynchronous on the main queue.
public final class BatchConversionCoordinator {
    private let engine: ConversionEngine
    public init(engine: ConversionEngine) { self.engine = engine }

    /// IDs must be unique; duplicate IDs are rejected before any engine call.
    public func convert(_ requests: [BatchConversionRequest], cancellation: BatchConversionCancellation,
                        progress: @escaping (BatchConversionProgress) -> Void,
                        completion: @escaping (Result<[BatchConversionItemResult], ConversionError>) -> Void) {
        DispatchQueue.main.async {
            guard Set(requests.map(\.id)).count == requests.count else {
                completion(.failure(ConversionError(code: .invalidRequest, message: "Batch item IDs must be unique")))
                return
            }
            let operation = BatchOperation(engine: self.engine, requests: requests, cancellation: cancellation,
                                           progress: progress, completion: completion)
            operation.start()
        }
    }
}

private final class BatchOperation {
    let engine: ConversionEngine
    let requests: [BatchConversionRequest]
    let cancellation: BatchConversionCancellation
    let progress: (BatchConversionProgress) -> Void
    let completion: (Result<[BatchConversionItemResult], ConversionError>) -> Void
    var results: [BatchConversionItemResult] = []

    init(engine: ConversionEngine, requests: [BatchConversionRequest], cancellation: BatchConversionCancellation,
         progress: @escaping (BatchConversionProgress) -> Void,
         completion: @escaping (Result<[BatchConversionItemResult], ConversionError>) -> Void) {
        self.engine = engine; self.requests = requests; self.cancellation = cancellation
        self.progress = progress; self.completion = completion
    }
    func emit(id: UUID? = nil, active: ConversionProgress? = nil) {
        progress(BatchConversionProgress(completed: results.count, total: requests.count, activeID: id, activeProgress: active))
    }
    func start() { emit(); scheduleNext() }
    func scheduleNext() {
        if cancellation.isCancelled {
            while results.count < requests.count {
                results.append(BatchConversionItemResult(id: requests[results.count].id, outcome: .cancelled))
                emit()
            }
        }
        guard results.count < requests.count else { completion(.success(results)); return }
        let item = requests[results.count]
        engine.convert(request: item.request, progress: { value in
            self.emit(id: item.id, active: value)
        }, completion: { result in
            let outcome: BatchConversionOutcome
            switch result {
            case .success(let value): outcome = .completed(value)
            case .failure(let error): outcome = .failed(error)
            }
            self.results.append(BatchConversionItemResult(id: item.id, outcome: outcome))
            self.emit()
            self.scheduleNext()
        })
    }
}
