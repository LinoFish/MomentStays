import XCTest
@testable import PhotoScan

private struct Source: PhotoScanSource {
    let access: PhotoAccess
    let values: [String: Data]
    let ids: [String]
    func requestAccess() async -> PhotoAccess { access }
    func assetIdentifiers() async throws -> [String] { ids }
    func originalResources(for identifier: String) async throws -> [PhotoOriginalResource] {
        guard let value = values[identifier] else { throw Failure.unavailable }
        return [PhotoOriginalResource(data: value, filename: identifier + ".heic", uniformTypeIdentifier: "public.heic")]
    }
}
private enum Failure: Error { case unavailable, recognition }

final class PhotoScannerTests: XCTestCase {
    func testMatchesOnlyPositiveAndReportsUnavailableAndErrors() async throws {
        let source = Source(access: .limited, values: ["motion": Data([1]), "still": Data([0]), "bad": Data([2])], ids: ["motion", "still", "cloud", "bad"])
        let scanner = PhotoScanner(source: source) { data in
            if data.data == Data([2]) { throw Failure.recognition }
            return data.data == Data([1])
        }
        let result = try await scanner.scan()
        XCTAssertEqual(result.access, .limited)
        XCTAssertEqual(result.motionAssetIdentifiers, ["motion"])
        XCTAssertEqual(result.matchedResources["motion"], PhotoResourceIdentity(filename: "motion.heic", uniformTypeIdentifier: "public.heic", resourceIndex: 0))
        XCTAssertEqual(result.issues, ["cloud": .originalUnavailable, "bad": .recognitionFailed])
    }

    func testOriginalMetadataAndBytesReachRecognizer() async throws {
        let source = Source(access: .authorized, values: ["photo": Data([0, 255, 42])], ids: ["photo"])
        let scanner = PhotoScanner(source: source) { resource in
            XCTAssertEqual(resource.data, Data([0, 255, 42]))
            XCTAssertEqual(resource.filename, "photo.heic")
            XCTAssertEqual(resource.uniformTypeIdentifier, "public.heic")
            XCTAssertEqual(resource.temporaryFileURL().pathExtension, "heic")
            return true
        }
        _ = try await scanner.scan()
    }

    func testProgressSnapshotsSurviveCancellation() async throws {
        actor Snapshots {
            var values: [PhotoScanProgress] = []
            func add(_ value: PhotoScanProgress) { values.append(value) }
        }
        let snapshots = Snapshots()
        let source = Source(access: .limited, values: ["a": Data([1]), "b": Data([1])], ids: ["a", "b"])
        let scanner = PhotoScanner(source: source) { _ in true }
        let task = Task {
            try await scanner.scan { value in
                await snapshots.add(value)
                if value.completed == 1 { withUnsafeCurrentTask { $0?.cancel() } }
            }
        }
        do { _ = try await task.value; XCTFail("Expected cancellation") } catch is CancellationError { }
        let latest = await snapshots.values.last
        XCTAssertEqual(latest?.access, .limited)
        XCTAssertEqual(latest?.motionAssetIdentifiers, ["a"])
        XCTAssertEqual(latest?.matchedResources["a"]?.filename, "a.heic")
    }

    func testDeniedDoesNotInspect() async throws {
        let scanner = PhotoScanner(source: Source(access: .denied, values: [:], ids: ["unreadable"])) { _ in
            XCTFail("Must not recognize without permission")
            return true
        }
        let result = try await scanner.scan()
        XCTAssertEqual(result.access, .denied)
        XCTAssertTrue(result.issues.isEmpty)
        XCTAssertTrue(result.motionAssetIdentifiers.isEmpty)
    }

    func testCancellationPropagates() async throws {
        let scanner = PhotoScanner(source: Source(access: .authorized, values: ["a": Data([1])], ids: ["a"])) { _ in
            try await Task.sleep(nanoseconds: 10_000_000_000)
            return true
        }
        let task = Task { try await scanner.scan() }
        task.cancel()
        do { _ = try await task.value; XCTFail("Expected cancellation") }
        catch is CancellationError { }
    }
}
