import 'dart:async';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:momentstays_mobile/native_service.dart';
import 'package:momentstays_mobile/service.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const methods = MethodChannel('momentstays/native');
  const events = MethodChannel('momentstays/batch');
  const scanEvents = MethodChannel('momentstays/scan');
  final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  const codec = StandardMethodCodec();

  Future<void> sendEvent(String channel, Object value) async {
    await messenger.handlePlatformMessage(channel, codec.encodeSuccessEnvelope(value), (_) {});
  }

  setUp(() {
    messenger.setMockMethodCallHandler(events, (_) async => null);
    messenger.setMockMethodCallHandler(scanEvents, (_) async => null);
  });
  tearDown(() {
    messenger.setMockMethodCallHandler(methods, null);
    messenger.setMockMethodCallHandler(events, null);
    messenger.setMockMethodCallHandler(scanEvents, null);
  });

  test('pending share preserves input direction and opaque identity', () async {
    messenger.setMockMethodCallHandler(methods, (call) async {
      expect(call.method, 'takePendingShare');
      return {'direction': 'liveToMotion', 'item': {'id': 'shared', 'name': 'item.heic'}};
    });
    final share = await NativeService().takePendingShare();
    expect(share!.direction, Direction.liveToMotion);
    expect(share.photo.id, 'shared');
  });

  test('scan progress is decoded and stop reaches the native bridge', () async {
    final calls = <String>[];
    messenger.setMockMethodCallHandler(methods, (call) async { calls.add(call.method); return null; });
    final service = NativeService();
    final progress = service.scanProgress.first;
    await Future<void>.delayed(Duration.zero);
    await sendEvent('momentstays/scan', {'completed': 2, 'total': 5, 'matches': 1, 'issues': 1});
    final value = await progress;
    expect([value.completed, value.total, value.matches, value.issues], [2, 5, 1, 1]);
    await service.stop();
    expect(calls, ['stop']);
  });

  test('batch updates preserve input and output identity', () async {
    messenger.setMockMethodCallHandler(methods, (call) async {
      if (call.method == 'convert') {
        Future<void>.delayed(Duration.zero, () => sendEvent('momentstays/batch', {
          'finished': true,
          'items': [{'id': 'a', 'name': 'a.jpg', 'status': 'success', 'outputId': 'out'}],
        }));
      }
      return null;
    });
    final photo = PhotoItem(id: 'a', name: 'a.jpg');
    final updates = await NativeService().convert(Direction.motionToLive, [photo], 'Google').toList();
    expect(updates.single.finished, isTrue);
    expect(updates.single.items.single.photo, same(photo));
    expect(updates.single.items.single.outputId, 'out');
  });

  test('event failure waits for authoritative native completion and recovers outputs', () async {
    final recovery = Completer<Object?>();
    final calls = <String>[];
    messenger.setMockMethodCallHandler(methods, (call) async {
      calls.add(call.method);
      if (call.method == 'convert') Future<void>.delayed(Duration.zero, () => messenger.handlePlatformMessage('momentstays/batch', codec.encodeErrorEnvelope(code: 'stream_error'), (_) {}));
      if (call.method == 'recoverBatch') return recovery.future;
      return null;
    });
    final photo = PhotoItem(id: 'a', name: 'a.jpg');
    final future = NativeService().convert(Direction.motionToLive, [photo], 'Google').toList();
    await Future<void>.delayed(const Duration(milliseconds: 20));
    expect(calls, containsAllInOrder(['convert', 'stop', 'recoverBatch']));
    recovery.complete({'finished': true, 'items': [{'id': 'a', 'name': 'a.jpg', 'status': 'success', 'outputId': 'a'}]});
    final result = await future;
    expect(result.single.items.single.outputId, 'a');
  });
  test('late native events after stream failure are ignored while recovering', () async {
    final startedRecovery = Completer<void>();
    final recovery = Completer<Object?>();
    messenger.setMockMethodCallHandler(methods, (call) async {
      if (call.method == 'convert') Future<void>.delayed(Duration.zero, () => messenger.handlePlatformMessage('momentstays/batch', codec.encodeErrorEnvelope(code: 'stream_error'), (_) {}));
      if (call.method == 'recoverBatch') { startedRecovery.complete(); return recovery.future; }
      return null;
    });
    final photo = PhotoItem(id: 'a', name: 'a.jpg');
    final future = NativeService().convert(Direction.motionToLive, [photo], 'Google').toList();
    await startedRecovery.future;
    await sendEvent('momentstays/batch', {'finished': false, 'items': [{'id': 'a', 'name': 'a.jpg', 'status': 'converting'}]});
    recovery.complete({'finished': true, 'items': [{'id': 'a', 'name': 'a.jpg', 'status': 'success', 'outputId': 'a'}]});
    expect((await future).single.items.single.outputId, 'a');
  });
  test('failed cleanup stop cannot mask recovery-unconfirmed exception', () async {
    var stops = 0;
    messenger.setMockMethodCallHandler(methods, (call) async {
      if (call.method == 'convert') Future<void>.delayed(Duration.zero, () => messenger.handlePlatformMessage('momentstays/batch', codec.encodeErrorEnvelope(code: 'stream_error'), (_) {}));
      if (call.method == 'stop') { stops++; throw PlatformException(code: 'stop_failed'); }
      return null;
    });
    final photo = PhotoItem(id: 'a', name: 'a.jpg');
    await expectLater(NativeService().convert(Direction.motionToLive, [photo], 'Google').toList(), throwsA(isA<RecoveryUnconfirmed>()));
    expect(stops, 2);
  });
  test('unsupported share error is not reported as success', () async {
    messenger.setMockMethodCallHandler(methods, (call) async {
      throw PlatformException(code: 'unsupported', message: 'Live Photo sharing unavailable');
    });
    final item = BatchItem(const PhotoItem(id: 'a', name: 'a.jpg'), ItemStatus.success, outputId: 'out');
    await expectLater(NativeService().share([item]), throwsA(isA<PlatformException>()));
  });
}
