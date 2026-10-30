import 'dart:typed_data';

enum Direction { motionToLive, liveToMotion }
enum Source { photos, files, scan }
enum ItemStatus { waiting, converting, success, failed, skipped }

class PhotoItem {
  const PhotoItem({required this.id, required this.name, this.thumbnail, this.unavailableReason});
  final String id, name;
  final Uint8List? thumbnail;
  /// Localized, user-visible reason supplied by the platform, or null if selectable.
  final String? unavailableReason;
}
class SharedSelection {
  const SharedSelection(this.direction, this.photo);
  final Direction direction;
  final PhotoItem photo;
}
class BatchItem {
  const BatchItem(this.photo, this.status, {this.reason, this.outputId});
  final PhotoItem photo;
  final ItemStatus status;
  final String? reason, outputId;
}
class BatchUpdate {
  const BatchUpdate(this.items, {this.finished = false});
  final List<BatchItem> items;
  final bool finished;
}
class ScanProgress {
  const ScanProgress(this.completed, this.total, this.matches, this.issues);
  final int completed, total, matches, issues;
}
/// Platform-owned IDs remain opaque. A finished update must include every input,
/// with unprocessed items marked skipped. Conversion errors are per-item.
abstract class MobileService {
  /// Claims a pending share-extension handoff, or returns null.
  Future<SharedSelection?> takePendingShare();
  /// Subscribe before starting a scan with pick. Results arrive from pick.
  Stream<ScanProgress> get scanProgress;
  Future<List<PhotoItem>> pick(Direction direction, Source source);
  Stream<BatchUpdate> convert(Direction direction, List<PhotoItem> items, String brand);
  /// Wait for native completion and recover the authoritative batch after a stream failure.
  Future<BatchUpdate> recoverConversion(List<PhotoItem> items);
  /// Requests cancellation of a scan or stops conversion after its current item.
  Future<void> stop();
  Future<void> share(List<BatchItem> items);
  Future<void> saveToPhotos(List<BatchItem> items);
  Future<void> saveToFiles(List<BatchItem> items);
  /// Release temporary inputs that are no longer needed, including unavailable items.
  Future<void> discardInputs(List<PhotoItem> items);
  Future<void> discard(List<BatchItem> items);
}
class ServiceUnavailable implements Exception {}
class RecoveryUnconfirmed implements Exception {}
class UnavailableService implements MobileService {
  const UnavailableService();
  @override Future<SharedSelection?> takePendingShare() async => null;
  @override Stream<ScanProgress> get scanProgress => const Stream.empty();
  Never unavailable() => throw ServiceUnavailable();
  @override Future<List<PhotoItem>> pick(Direction direction, Source source) async => unavailable();
  @override Stream<BatchUpdate> convert(Direction direction, List<PhotoItem> items, String brand) => Stream.error(ServiceUnavailable());
  @override Future<BatchUpdate> recoverConversion(List<PhotoItem> items) async => unavailable();
  @override Future<void> stop() async => unavailable();
  @override Future<void> share(List<BatchItem> items) async => unavailable();
  @override Future<void> saveToPhotos(List<BatchItem> items) async => unavailable();
  @override Future<void> saveToFiles(List<BatchItem> items) async => unavailable();
  @override Future<void> discardInputs(List<PhotoItem> items) async => unavailable();
  @override Future<void> discard(List<BatchItem> items) async => unavailable();
}
