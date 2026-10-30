import 'dart:async';
import 'package:flutter/services.dart';
import 'service.dart';

/// iOS bridge. Capability keys describe implemented operations, not permission status.
class NativeService implements MobileService {
  NativeService({MethodChannel? methods, EventChannel? events})
      : _methods = methods ?? const MethodChannel('momentstays/native'),
        _events = events ?? const EventChannel('momentstays/batch');
  final MethodChannel _methods;
  final EventChannel _events;
  bool _running = false;

  /// Subscribe before invoking pick(..., Source.scan). Stop cancels a scan.
  @override
  Stream<ScanProgress> get scanProgress => const EventChannel('momentstays/scan')
      .receiveBroadcastStream().map((event) {
        final value = Map<dynamic, dynamic>.from(event as Map);
        return ScanProgress(value['completed'] as int, value['total'] as int,
            value['matches'] as int, value['issues'] as int);
      });

  Future<Map<String, bool>> capabilities() async {
    final value = await _methods.invokeMapMethod<String, dynamic>('capabilities');
    return (value ?? {}).map((key, value) => MapEntry(key, value == true));
  }

  @override
  Future<SharedSelection?> takePendingShare() async {
    final result = await _methods.invokeMapMethod<String, dynamic>('takePendingShare');
    if (result == null) return null;
    final item = Map<dynamic, dynamic>.from(result['item'] as Map);
    return SharedSelection(Direction.values.byName(result['direction'] as String),
        PhotoItem(id: item['id'] as String, name: item['name'] as String));
  }

  @override
  Future<List<PhotoItem>> pick(Direction direction, Source source) async {
    final rows = await _methods.invokeListMethod<dynamic>('pick', {'direction': direction.name, 'source': source.name}) ?? [];
    return rows.map((v) { final m = Map<dynamic, dynamic>.from(v as Map); return PhotoItem(id: m['id'] as String, name: m['name'] as String, thumbnail: m['thumbnail'] as Uint8List?, unavailableReason: m['unavailableReason'] as String?); }).toList();
  }

  @override
  Stream<BatchUpdate> convert(Direction direction, List<PhotoItem> items, String brand) async* {
    if (_running) throw StateError('A batch is already running');
    _running = true;
    final controller = StreamController<BatchUpdate>();
    late StreamSubscription<dynamic> subscription;
    final originals = {for (final item in items) item.id: item};
    var started = false;
    var finished = false;
    subscription = _events.receiveBroadcastStream().listen((event) {
      try {
        final map = Map<dynamic, dynamic>.from(event as Map);
        final rows = (map['items'] as List).map((row) {
          final m = Map<dynamic, dynamic>.from(row as Map);
          final photo = originals[m['id']] ?? PhotoItem(id: m['id'] as String, name: m['name'] as String);
          return BatchItem(photo, ItemStatus.values.byName(m['status'] as String), reason: m['reason'] as String?, outputId: m['outputId'] as String?);
        }).toList();
        finished = map['finished'] == true;
        controller.add(BatchUpdate(rows, finished: finished));
        if (finished) controller.close();
      } catch (e, stack) { controller.addError(e, stack); controller.close(); }
    }, onError: (Object e, StackTrace stack) { controller.addError(e, stack); controller.close(); });
    try {
      await _methods.invokeMethod<void>('convert', {'direction': direction.name, 'ids': items.map((e) => e.id).toList(), 'brand': brand});
      started = true;
      yield* controller.stream;
    } finally {
      // Abandoning a stream requests stop; the native current item may still finish.
      try {
        if (started && !finished) await _methods.invokeMethod<void>('stop');
      } finally {
        await subscription.cancel();
        if (!controller.isClosed) unawaited(controller.close());
        _running = false;
      }
    }
  }
  List<String> _outputs(List<BatchItem> items) {
    if (items.isEmpty || items.any((item) => item.status != ItemStatus.success || item.outputId == null)) {
      throw ArgumentError('Every item must be a successful conversion with an output.');
    }
    return items.map((e) => e.outputId!).toList();
  }
  @override Future<void> stop() => _methods.invokeMethod<void>('stop');
  @override Future<void> share(List<BatchItem> items) => _methods.invokeMethod<void>('share', {'ids': _outputs(items)});
  @override Future<void> saveToPhotos(List<BatchItem> items) => _methods.invokeMethod<void>('saveToPhotos', {'ids': _outputs(items)});
  @override Future<void> saveToFiles(List<BatchItem> items) => _methods.invokeMethod<void>('saveToFiles', {'ids': _outputs(items)});
  @override Future<void> discardInputs(List<PhotoItem> items) => _methods.invokeMethod<void>('discard', {'ids': items.map((e) => e.id).toList()});
  @override Future<void> discard(List<BatchItem> items) => _methods.invokeMethod<void>('discard', {'ids': [...items.map((e) => e.photo.id), ...items.map((e) => e.outputId).whereType<String>()]});
}
