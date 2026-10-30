import Foundation
import Testing
@testable import MomentStays

private struct BatchMockEngine: ConversionEngine {
    func inspect(input: URL, cancellation: InspectionCancellation, completion: @escaping (Result<InspectionResult, ConversionError>) -> Void) {
        DispatchQueue.main.async { completion(.success(InspectionResult(classification: input.lastPathComponent == "bad.jpg" ? .unknown : .motion, detail: ""))) }
    }
    func convert(request: ConversionRequest, progress: @escaping (ConversionProgress) -> Void, completion: @escaping (Result<ConversionResult, ConversionError>) -> Void) {
        DispatchQueue.main.async {
            do {
                let output = request.outputDirectory.appendingPathComponent("published", isDirectory: true)
                try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
                try Data().write(to: output.appendingPathComponent("photo.jpg"))
                try Data().write(to: output.appendingPathComponent("video.mov"))
                completion(.success(ConversionResult(items: [ConversionItem(source: request.inputs[0], output: output, status: .converted, detail: "")], report: nil)))
            } catch { completion(.failure(ConversionError(code: .failed, message: error.localizedDescription))) }
        }
    }
}

struct BatchWorkTests {
    @Test @MainActor func folderIsNonrecursiveAndFailuresDoNotStopOthers() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        for name in ["bad.jpg", "good.HEIC", "ignore.png"] { try Data().write(to: root.appendingPathComponent(name)) }
        let subfolder = root.appendingPathComponent("nested", isDirectory: true)
        try FileManager.default.createDirectory(at: subfolder, withIntermediateDirectories: true)
        try Data().write(to: subfolder.appendingPathComponent("nested.jpg"))
        try FileManager.default.createSymbolicLink(at: root.appendingPathComponent("link.jpg"), withDestinationURL: root.appendingPathComponent("bad.jpg"))
        let folder = BatchFolderAccess(url: root)
        let files = try folder.files()
        #expect(files.map(\.lastPathComponent) == ["bad.jpg", "good.HEIC"])
        let work = BatchWork(inputs: files.map { BatchInput(name: $0.lastPathComponent, source: .folderFile($0)) }, engine: BatchMockEngine(), folder: folder)
        await work.start(brand: "Google")
        #expect(work.completed == 2)
        if case .failed = work.rows[0].state {} else { Issue.record("Invalid original should fail") }
        if case .converted(let result) = work.rows[1].state { #expect(result.files.count == 2) } else { Issue.record("Later valid original should convert") }
        work.discard()
    }
}

private final class DelayedInspectionEngine: ConversionEngine {
    var completion: ((Result<InspectionResult, ConversionError>) -> Void)?
    var inspectedURL: URL?
    func inspect(input: URL, cancellation: InspectionCancellation, completion: @escaping (Result<InspectionResult, ConversionError>) -> Void) {
        inspectedURL = input
        self.completion = completion
    }
    func convert(request: ConversionRequest, progress: @escaping (ConversionProgress) -> Void, completion: @escaping (Result<ConversionResult, ConversionError>) -> Void) {
        Issue.record("Conversion should not start")
    }
}

extension BatchWorkTests {
    @Test @MainActor func leavingDuringPreparationCleansStagedFiles() async throws {
        let source = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".jpg")
        try Data().write(to: source)
        defer { try? FileManager.default.removeItem(at: source) }
        let engine = DelayedInspectionEngine()
        let work = BatchWork(inputs: [BatchInput(name: "photo.jpg", source: .folderFile(source))], engine: engine)
        let task = Task { await work.checkInputs() }
        while engine.completion == nil { await Task.yield() }
        let staged = try #require(engine.inspectedURL)
        #expect(FileManager.default.fileExists(atPath: staged.path))
        work.discardWhenIdle()
        engine.completion?(.success(InspectionResult(classification: .motion, detail: "")))
        await task.value
        #expect(!FileManager.default.fileExists(atPath: staged.path))
    }
}

private final class StopDuringPreparationEngine: ConversionEngine {
    var inspectCompletion: ((Result<InspectionResult, ConversionError>) -> Void)?
    var conversionCount = 0
    func inspect(input: URL, cancellation: InspectionCancellation, completion: @escaping (Result<InspectionResult, ConversionError>) -> Void) {
        inspectCompletion = completion
    }
    func convert(request: ConversionRequest, progress: @escaping (ConversionProgress) -> Void, completion: @escaping (Result<ConversionResult, ConversionError>) -> Void) {
        conversionCount += 1
        do {
            let folder = request.outputDirectory.appendingPathComponent("out", isDirectory: true)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try Data().write(to: folder.appendingPathComponent("photo.jpg"))
            try Data().write(to: folder.appendingPathComponent("video.mov"))
            completion(.success(ConversionResult(items: [ConversionItem(source: request.inputs[0], output: folder, status: .converted, detail: "")], report: nil)))
        } catch { completion(.failure(ConversionError(code: .failed, message: error.localizedDescription))) }
    }
}

extension BatchWorkTests {
    @Test @MainActor func stopDuringPreparationCompletesCurrentAndSkipsRest() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let urls = ["first.jpg", "second.jpg"].map { root.appendingPathComponent($0) }
        for url in urls { try Data().write(to: url) }
        let engine = StopDuringPreparationEngine()
        let work = BatchWork(inputs: urls.map { BatchInput(name: $0.lastPathComponent, source: .folderFile($0)) }, engine: engine)
        let task = Task { await work.start(brand: "Google") }
        while engine.inspectCompletion == nil { await Task.yield() }
        work.stopAfterCurrent()
        engine.inspectCompletion?(.success(InspectionResult(classification: .motion, detail: "")))
        await task.value
        #expect(engine.conversionCount == 1)
        if case .converted = work.rows[0].state {} else { Issue.record("Current item should finish") }
        if case .cancelled = work.rows[1].state {} else { Issue.record("Later item should be skipped") }
        work.discard()
    }
}
