import Foundation

struct ConversionJob {
    let sources: [URL]
    let output: URL
    let options: ConversionOptions
    var scanRows: [ScanRow]? = nil
}

typealias ExistingOutputPresenter = (URL, URL, @escaping (ExistingOutputDecision) -> Void) -> Void

// Completion/progress/presentation callbacks are delivered on the main queue.
// Implementations own execution, never the lifetime of a SwiftUI View.
protocol ConversionServing {
    func scan(sources: [URL], direction: ConversionDirection, token: StopToken, rowsReady: @escaping ([ScanRow]) -> Void,
              progress: @escaping (Int, Int) -> Void, completion: @escaping (Result<[ScanRow], Error>) -> Void)
    func run(job: ConversionJob, token: StopToken, existingOutput: @escaping ExistingOutputPresenter,
             progress: @escaping (BatchProgress) -> Void, completion: @escaping (Result<BatchOutcome, Error>) -> Void)
}

// Safe even if the user cancels while the platform sheet is still being presented.
// Cancellation can release the worker without depending on a sheet's callback.
final class OutputDecisionLatch {
    private let lock = NSLock()
    private let semaphore = DispatchSemaphore(value: 0)
    private var decision: ExistingOutputDecision?

    func resolve(_ value: ExistingOutputDecision) {
        lock.lock()
        guard decision == nil else { lock.unlock(); return }
        decision = value
        lock.unlock()
        semaphore.signal()
    }

    func wait(token: StopToken) -> ExistingOutputDecision {
        while semaphore.wait(timeout: .now() + 0.1) == .timedOut {
            if token.isCancelled { resolve(.cancel) }
        }
        lock.lock(); defer { lock.unlock() }
        return token.isCancelled ? .cancel : decision ?? .cancel
    }
}

final class ConversionService: ConversionServing {
    private let worker = DispatchQueue(label: "app.motionlive.conversion", qos: .userInitiated)

    func scan(sources: [URL], direction: ConversionDirection, token: StopToken, rowsReady: @escaping ([ScanRow]) -> Void,
              progress: @escaping (Int, Int) -> Void, completion: @escaping (Result<[ScanRow], Error>) -> Void) {
        worker.async {
            var lastUpdate = Date.distantPast
            let result = Result {
                try NativeBatch.scan(sources: sources, direction: direction.rawValue, token: token, rowsReady: { rows in
                    DispatchQueue.main.async { rowsReady(rows) }
                }) { index, total in
                    let now = Date()
                    if index == total || now.timeIntervalSince(lastUpdate) >= 0.1 {
                        lastUpdate = now
                        DispatchQueue.main.async { progress(index, total) }
                    }
                }
            }
            DispatchQueue.main.async { completion(result) }
        }
    }

    func run(job: ConversionJob, token: StopToken, existingOutput: @escaping ExistingOutputPresenter,
             progress: @escaping (BatchProgress) -> Void, completion: @escaping (Result<BatchOutcome, Error>) -> Void) {
        worker.async {
            let result = Result {
                try NativeBatch.run(sources: job.sources, output: job.output, options: job.options, token: token, scanRows: job.scanRows,
                    existingOutput: { source, target in
                        let latch = OutputDecisionLatch()
                        DispatchQueue.main.async {
                            guard !token.isCancelled else { latch.resolve(.cancel); return }
                            existingOutput(source, target) { latch.resolve($0) }
                        }
                        return latch.wait(token: token)
                    }) { update in
                        // Every per-file result matters; don't drop terminal rows when batching progress.
                        DispatchQueue.main.async { progress(update) }
                    }
            }
            DispatchQueue.main.async { completion(result) }
        }
    }
}
