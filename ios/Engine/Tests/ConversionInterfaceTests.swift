import Foundation
import XCTest
@testable import ConversionInterface

private final class MockEngine: ConversionEngine {
    var received: ConversionRequest?
    var inspected: URL?
    var inspection: Result<InspectionResult, ConversionError> = .success(InspectionResult(classification: .unknown, detail: "unrecognized"))
    let response: Result<ConversionResult, ConversionError>
    init(_ response: Result<ConversionResult, ConversionError>) { self.response = response }
    func inspect(input: URL, cancellation: InspectionCancellation,
                 completion: @escaping (Result<InspectionResult, ConversionError>) -> Void) {
        inspected = input
        DispatchQueue.main.async {
            if cancellation.isCancelled {
                completion(.failure(ConversionError(code: .cancelled, message: "cancelled")))
            } else { completion(self.inspection) }
        }
    }
    func convert(request: ConversionRequest, progress: @escaping (ConversionProgress) -> Void,
                 completion: @escaping (Result<ConversionResult, ConversionError>) -> Void) {
        received = request
        DispatchQueue.main.async {
            progress(ConversionProgress(completed: 1, total: 1, item: "input.jpg", status: "converted", detail: "done"))
            completion(self.response)
        }
    }
}

final class ConversionInterfaceTests: XCTestCase {
    func testRequestProgressAndResultThroughProtocol() {
        let input = URL(fileURLWithPath: "/input.jpg"), output = URL(fileURLWithPath: "/output")
        let item = ConversionItem(source: input, output: output, status: .converted, detail: "done")
        let mock = MockEngine(.success(ConversionResult(items: [item], report: nil)))
        let engine: ConversionEngine = mock
        let finished = expectation(description: "completion")
        var events: [String] = []
        engine.convert(request: ConversionRequest(inputs: [input], outputDirectory: output, targetVendor: .samsung), progress: { value in
            XCTAssertTrue(Thread.isMainThread); XCTAssertEqual(value.completed, 1); XCTAssertEqual(value.total, 1)
            XCTAssertEqual(value.item, "input.jpg"); events.append("progress")
        }, completion: { result in
            XCTAssertTrue(Thread.isMainThread); events.append("completion")
            guard case .success(let value) = result else { return XCTFail("expected success") }
            XCTAssertEqual(value.items.first?.output, output); XCTAssertEqual(value.items.first?.status, .converted)
            XCTAssertEqual(events, ["progress", "completion"]); finished.fulfill()
        })
        XCTAssertEqual(mock.received?.inputs, [input]); XCTAssertEqual(mock.received?.outputDirectory, output)
        XCTAssertEqual(mock.received?.targetVendor, .samsung)
        wait(for: [finished], timeout: 2)
    }

    func testInspectionClassificationErrorsAndCancellation() {
        let file = URL(fileURLWithPath: "/original.jpg")
        let mock = MockEngine(.success(ConversionResult(items: [], report: nil)))
        let engine: ConversionEngine = mock
        let finished = expectation(description: "inspection cases")
        finished.expectedFulfillmentCount = 4
        for classification in [InspectionClassification.motion, .staticImage, .unknown] {
            mock.inspection = .success(InspectionResult(classification: classification, detail: "evidence"))
            // Separate mocks avoid mutating an in-flight response.
            let individual = MockEngine(.success(ConversionResult(items: [], report: nil)))
            individual.inspection = mock.inspection
            let inspector: ConversionEngine = individual
            inspector.inspect(input: file, cancellation: InspectionCancellation()) { result in
                XCTAssertTrue(Thread.isMainThread)
                guard case .success(let value) = result else { return XCTFail("expected classification") }
                XCTAssertEqual(value.classification, classification); XCTAssertEqual(value.detail, "evidence")
                finished.fulfill()
            }
            XCTAssertEqual(individual.inspected, file)
        }
        let token = InspectionCancellation(); token.cancel()
        engine.inspect(input: file, cancellation: token) { result in
            guard case .failure(let error) = result else { return XCTFail("expected cancellation") }
            XCTAssertEqual(error.code, .cancelled); finished.fulfill()
        }
        wait(for: [finished], timeout: 2)
        mock.inspection = .failure(ConversionError(code: .failed, message: "unreadable"))
        let failed = expectation(description: "inspection error")
        engine.inspect(input: file, cancellation: InspectionCancellation()) { result in
            guard case .failure(let error) = result else { return XCTFail("expected error") }
            XCTAssertEqual(error.code, .failed); XCTAssertEqual(error.message, "unreadable"); failed.fulfill()
        }
        wait(for: [failed], timeout: 2)
    }

    func testErrorPassesThroughProtocol() {
        let engine: ConversionEngine = MockEngine(.failure(ConversionError(code: .failed, message: "bad input")))
        let finished = expectation(description: "failure")
        engine.convert(request: ConversionRequest(inputs: [], outputDirectory: URL(fileURLWithPath: "/out"), targetVendor: .apple), progress: { _ in }, completion: { result in
            guard case .failure(let error) = result else { return XCTFail("expected failure") }
            XCTAssertEqual(error.code, .failed); XCTAssertEqual(error.localizedDescription, "bad input"); finished.fulfill()
        })
        wait(for: [finished], timeout: 2)
    }
}
