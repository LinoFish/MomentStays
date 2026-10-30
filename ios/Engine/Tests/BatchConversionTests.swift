import Foundation
import XCTest
@testable import ConversionInterface

private final class ControlledEngine: ConversionEngine {
    var started: [ConversionRequest] = []
    var onStart: (() -> Void)?
    var progress: ((ConversionProgress) -> Void)?
    var finish: ((Result<ConversionResult, ConversionError>) -> Void)?
    func inspect(input: URL, cancellation: InspectionCancellation, completion: @escaping (Result<InspectionResult, ConversionError>) -> Void) {}
    func convert(request: ConversionRequest, progress: @escaping (ConversionProgress) -> Void,
                 completion: @escaping (Result<ConversionResult, ConversionError>) -> Void) {
        started.append(request); self.progress = progress; finish = completion; onStart?()
    }
}

final class BatchConversionTests: XCTestCase {
    private func item(_ name: String) -> BatchConversionRequest {
        BatchConversionRequest(request: ConversionRequest(inputs: [URL(fileURLWithPath: "/\(name)")], outputDirectory: URL(fileURLWithPath: "/out"), targetVendor: .apple))
    }
    func testMixedResultsAndProgress() {
        let engine = ControlledEngine(), coordinator = BatchConversionCoordinator(engine: engine)
        let items = [item("a"), item("b"), item("c")], done = expectation(description: "done")
        let failedItem = ConversionItem(source: items[2].request.inputs[0], output: nil, status: .failed, detail: "item failure")
        var counts: [Int] = []; var active: [UUID] = []
        engine.onStart = {
            let index = engine.started.count - 1
            XCTAssertEqual(engine.started[index].inputs, items[index].request.inputs)
            DispatchQueue.main.async {
                engine.progress?(ConversionProgress(completed: 99, total: 0, item: "x", status: "working", detail: ""))
                if index == 1 { engine.finish?(.failure(ConversionError(code: .failed, message: "bad"))) }
                else { engine.finish?(.success(ConversionResult(items: index == 2 ? [failedItem] : [], report: nil))) }
            }
        }
        coordinator.convert(items, cancellation: BatchConversionCancellation(), progress: { p in
            XCTAssertTrue(Thread.isMainThread); XCTAssertEqual(p.total, 3); counts.append(p.completed)
            if let id = p.activeID { active.append(id); XCTAssertEqual(p.activeProgress?.total, 0) }
        }, completion: { result in
            guard case .success(let values) = result else { return XCTFail("batch failed") }
            XCTAssertEqual(values.map(\.id), items.map(\.id))
            guard case .failed(let error) = values[1].outcome else { return XCTFail("missing failure") }
            XCTAssertEqual(error.message, "bad")
            guard case .completed(let value) = values[2].outcome else { return XCTFail("missing result") }
            XCTAssertEqual(value.items[0].status, .failed)
            XCTAssertEqual(counts, [0, 0, 1, 1, 2, 2, 3]); XCTAssertEqual(active, items.map(\.id)); done.fulfill()
        })
        wait(for: [done], timeout: 2)
    }
    func testCancellationBeforeAndDuringConversion() {
        for during in [false, true] {
            let engine = ControlledEngine(), token = BatchConversionCancellation()
            let items = [item("a"), item("b")], done = expectation(description: "cancel")
            if !during { token.cancel() }
            engine.onStart = {
                token.cancel()
                DispatchQueue.main.async { engine.finish?(.success(ConversionResult(items: [], report: nil))) }
            }
            var counts: [Int] = []
            BatchConversionCoordinator(engine: engine).convert(items, cancellation: token, progress: { counts.append($0.completed) }, completion: { result in
                guard case .success(let values) = result else { return XCTFail("batch failed") }
                XCTAssertEqual(engine.started.count, during ? 1 : 0)
                if during { guard case .completed = values[0].outcome else { return XCTFail("in-flight lost") } }
                else { guard case .cancelled = values[0].outcome else { return XCTFail("started") } }
                guard case .cancelled = values[1].outcome else { return XCTFail("started") }
                XCTAssertEqual(counts, [0, 1, 2]); done.fulfill()
            })
            wait(for: [done], timeout: 2)
        }
    }
    func testEmptyAndDuplicateIDs() {
        let engine = ControlledEngine(), coordinator = BatchConversionCoordinator(engine: engine)
        let empty = expectation(description: "empty"), duplicate = expectation(description: "duplicate")
        coordinator.convert([], cancellation: BatchConversionCancellation(), progress: { XCTAssertEqual($0.total, 0) }, completion: { result in
            guard case .success(let items) = result else { return XCTFail("empty failed") }; XCTAssertTrue(items.isEmpty); empty.fulfill()
        })
        let request = item("a")
        coordinator.convert([request, request], cancellation: BatchConversionCancellation(), progress: { _ in XCTFail("unexpected progress") }, completion: { result in
            guard case .failure(let error) = result else { return XCTFail("accepted duplicate") }; XCTAssertEqual(error.code, .invalidRequest); duplicate.fulfill()
        })
        wait(for: [empty, duplicate], timeout: 2); XCTAssertTrue(engine.started.isEmpty)
    }
}
