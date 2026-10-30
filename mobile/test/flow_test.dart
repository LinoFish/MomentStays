import 'dart:async';
import 'package:flutter/cupertino.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:momentstays_mobile/main.dart';
import 'package:momentstays_mobile/service.dart';

class FakeService extends UnavailableService {
  @override Future<List<PhotoItem>> pick(Direction direction, Source source) async => const [PhotoItem(id: '1', name: 'one.jpg'), PhotoItem(id: '2', name: 'bad.jpg', unavailableReason: 'Not a Motion Photo')];
  @override Stream<BatchUpdate> convert(Direction direction, List<PhotoItem> items, String brand) async* {
    yield BatchUpdate(items.map((p) => BatchItem(p, ItemStatus.failed, reason: 'Failed')).toList(), finished: true);
  }
  @override Future<void> discard(List<BatchItem> items) async {}
}
class ScanFake extends UnavailableService {
  final progress = StreamController<ScanProgress>.broadcast(sync: true);
  final result = Completer<List<PhotoItem>>();
  bool stopped = false;
  @override Stream<ScanProgress> get scanProgress => progress.stream;
  @override Future<List<PhotoItem>> pick(Direction direction, Source source) => result.future;
  @override Future<void> stop() async { stopped = true; }
}
class SuccessFake extends UnavailableService {
  final saved = Completer<void>();
  final discarded = <String>[];
  int resultDiscards = 0;
  @override Future<List<PhotoItem>> pick(Direction direction, Source source) async => const [PhotoItem(id: '1', name: 'one.jpg'), PhotoItem(id: '2', name: 'two.jpg')];
  @override Stream<BatchUpdate> convert(Direction direction, List<PhotoItem> items, String brand) async* {
    yield BatchUpdate(items.map((p) => BatchItem(p, ItemStatus.success, outputId: 'out-${p.id}')).toList(), finished: true);
  }
  @override Future<void> discardInputs(List<PhotoItem> items) async { discarded.addAll(items.map((p) => p.id)); }
  @override Future<void> discard(List<BatchItem> items) async { resultDiscards++; }
  @override Future<void> saveToPhotos(List<BatchItem> items) => saved.future;
}
class RetrySaveFake extends UnavailableService {
  final saves = <List<String>>[];
  @override Future<List<PhotoItem>> pick(Direction direction, Source source) async => const [PhotoItem(id: '1', name: 'one.jpg'), PhotoItem(id: '2', name: 'two.jpg')];
  @override Stream<BatchUpdate> convert(Direction direction, List<PhotoItem> items, String brand) async* {
    yield BatchUpdate(items.map((p) => p.id == '2' && items.length > 1 ? BatchItem(p, ItemStatus.failed, reason: 'error') : BatchItem(p, ItemStatus.success, outputId: 'out-${p.id}')).toList(), finished: true);
  }
  @override Future<void> saveToPhotos(List<BatchItem> items) async { saves.add(items.map((e) => e.outputId!).toList()); }
  @override Future<void> discard(List<BatchItem> items) async {}
}
class GlobalErrorFake extends FakeService {
  @override Stream<BatchUpdate> convert(Direction direction, List<PhotoItem> items, String brand) => Stream.error(StateError('backend failure'));
}
class DeferredPickFake extends UnavailableService {
  final picked = Completer<List<PhotoItem>>();
  final discarded = <String>[];
  int calls = 0;
  @override Future<List<PhotoItem>> pick(Direction direction, Source source) { calls++; return picked.future; }
  @override Future<void> discardInputs(List<PhotoItem> items) async { discarded.addAll(items.map((e) => e.id)); }
}
class DeferredScanFake extends DeferredPickFake {
  final events = StreamController<ScanProgress>.broadcast();
  int stops = 0;
  @override Stream<ScanProgress> get scanProgress => events.stream;
  @override Future<void> stop() async { stops++; }
}
class StopRaceFake extends FakeService {
  final updates = StreamController<BatchUpdate>.broadcast(sync: true);
  int stops = 0;
  @override Stream<BatchUpdate> convert(Direction direction, List<PhotoItem> items, String brand) => updates.stream;
  @override Future<void> stop() async { stops++; }
}
class UnavailablePickFake extends FakeService {
  @override Future<List<PhotoItem>> pick(Direction direction, Source source) async => const [PhotoItem(id: 'bad', name: 'bad.jpg', unavailableReason: 'Not usable')];
}
class CancelRaceFake extends FakeService {
  final release = Completer<void>();
  late final StreamController<BatchUpdate> first = StreamController<BatchUpdate>(sync: true, onCancel: () => release.future);
  int conversions = 0;
  @override Stream<BatchUpdate> convert(Direction direction, List<PhotoItem> items, String brand) {
    conversions++;
    if (conversions == 1) return first.stream;
    return Stream.value(BatchUpdate(items.map((p) => BatchItem(p, ItemStatus.success, outputId: 'out')).toList(), finished: true));
  }
}
class CleanupRaceFake extends SuccessFake {
  final updates = StreamController<BatchUpdate>(sync: true);
  int stops = 0;
  final disposedBatch = <BatchItem>[];
  @override Stream<BatchUpdate> convert(Direction direction, List<PhotoItem> items, String brand) => updates.stream;
  @override Future<void> stop() async { stops++; }
  @override Future<void> discard(List<BatchItem> items) async { disposedBatch.addAll(items); }
}
class ScanStopRaceFake extends UnavailableService {
  final scans = [Completer<List<PhotoItem>>(), Completer<List<PhotoItem>>()];
  final stopResult = Completer<void>();
  final events = StreamController<ScanProgress>.broadcast();
  int calls = 0;
  @override Stream<ScanProgress> get scanProgress => events.stream;
  @override Future<List<PhotoItem>> pick(Direction direction, Source source) => scans[calls++].future;
  @override Future<void> stop() => stopResult.future;
  @override Future<void> discardInputs(List<PhotoItem> items) async {}
}
class PartialCleanupFake extends SuccessFake {
  int inputAttempts = 0;
  @override Future<void> discardInputs(List<PhotoItem> items) async { inputAttempts++; if (inputAttempts == 1) throw StateError('temporary'); await super.discardInputs(items); }
}
void main() {
  testWidgets('partial result cleanup stays recoverable without discarding results twice', (tester) async {
    final service = PartialCleanupFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files')); await tester.pumpAndSettle();
    await tester.tap(find.text('two.jpg')); await tester.pump();
    await tester.tap(find.text('Convert 1 Photo')); await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byType(CupertinoNavigationBar), matching: find.widgetWithText(CupertinoButton, 'Done'))); await tester.pumpAndSettle();
    await tester.tap(find.text('Leave')); await tester.pumpAndSettle();
    await tester.tap(find.text('OK')); await tester.pumpAndSettle();
    expect(find.text('Results were cleared, but some inputs remain. Tap Done to retry cleanup.'), findsOneWidget);
    await tester.tap(find.descendant(of: find.byType(CupertinoNavigationBar), matching: find.widgetWithText(CupertinoButton, 'Done'))); await tester.pumpAndSettle();
    expect(service.resultDiscards, 1); expect(service.discarded, ['2']);
  });
  testWidgets('late stop failure does not affect a later scan', (tester) async {
    final service = ScanStopRaceFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Scan Library')); await tester.pump(); await tester.pump(const Duration(seconds: 1));
    await tester.tap(find.text('Cancel Scan')); await tester.pump();
    service.scans[0].complete([]); await tester.pumpAndSettle();
    await tester.tap(find.text('Scan Again')); await tester.pump(); await tester.pump(const Duration(seconds: 1)); await tester.pump();
    expect(service.calls, 2);
    service.stopResult.completeError(StateError('late stop')); await tester.pump();
    expect(find.text('Checking photos'), findsOneWidget);
    expect(find.text('Operation Could Not Be Completed'), findsNothing);
    service.scans[1].complete([]); await tester.pumpAndSettle(); await service.events.close();
  });
  testWidgets('disposing selection discards held inputs', (tester) async {
    final service = SuccessFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files')); await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox()); await tester.pump(); await tester.pump();
    expect(service.discarded, ['1', '2']);
  });
  testWidgets('disposing conversion waits for final outputs before discard', (tester) async {
    final service = CleanupRaceFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files')); await tester.pumpAndSettle();
    await tester.tap(find.text('Convert 2 Photos')); await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox()); await tester.pump();
    expect(service.stops, 1); expect(service.disposedBatch, isEmpty);
    service.updates.add(const BatchUpdate([BatchItem(PhotoItem(id: '1', name: 'one.jpg'), ItemStatus.success, outputId: 'out-1'), BatchItem(PhotoItem(id: '2', name: 'two.jpg'), ItemStatus.skipped)], finished: true));
    unawaited(service.updates.close()); await tester.pump(); await tester.pump();
    expect(service.disposedBatch.map((e) => e.outputId), contains('out-1'));
  });
  testWidgets('disposing while saving waits before discarding output', (tester) async {
    final service = SuccessFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files')); await tester.pumpAndSettle();
    await tester.tap(find.text('Convert 2 Photos')); await tester.pumpAndSettle();
    await tester.tap(find.text('Save 2 to Photos')); await tester.pump();
    await tester.pumpWidget(const SizedBox()); await tester.pump();
    expect(service.resultDiscards, 0);
    service.saved.complete(); await tester.pump(); await tester.pump();
    expect(service.resultDiscards, 1);
  });
  testWidgets('home row labels resolve for dark appearance and large text', (tester) async {
    tester.platformDispatcher.platformBrightnessTestValue = Brightness.dark;
    tester.platformDispatcher.textScaleFactorTestValue = 2;
    addTearDown(() { tester.platformDispatcher.clearPlatformBrightnessTestValue(); tester.platformDispatcher.clearTextScaleFactorTestValue(); });
    await tester.pumpWidget(const MomentStaysApp()); await tester.pumpAndSettle();
    final label = tester.widget<Text>(find.text('Android → Live Photo'));
    expect(label.style!.color!.toARGB32(), CupertinoColors.label.darkColor.toARGB32());
    expect(tester.takeException(), isNull);
  });
  testWidgets('retry waits for previous stream cancellation', (tester) async {
    final service = CancelRaceFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files')); await tester.pumpAndSettle();
    await tester.tap(find.text('Convert 1 Photo')); await tester.pumpAndSettle();
    service.first.add(const BatchUpdate([BatchItem(PhotoItem(id: '1', name: 'one.jpg'), ItemStatus.failed)], finished: true));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Retry 1 Failed Photo')); await tester.pump();
    expect(service.conversions, 1);
    service.release.complete(); await tester.pump(); await tester.pumpAndSettle();
    expect(service.conversions, 2);
  });
  testWidgets('late picker results are discarded on dispose', (tester) async {
    final service = DeferredPickFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files')); await tester.pump();
    expect(service.calls, 1);
    await tester.pumpWidget(const SizedBox());
    service.picked.complete(const [PhotoItem(id: 'private', name: 'private.jpg')]);
    await tester.pump(); await tester.pump();
    expect(service.discarded, ['private']);
  });
  testWidgets('late scan results are discarded after disposal', (tester) async {
    final service = DeferredScanFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Scan Library')); await tester.pump();
    await tester.pumpWidget(const SizedBox());
    service.picked.complete(const [PhotoItem(id: 'late', name: 'late.jpg')]);
    await tester.pump(); await tester.pump();
    expect(service.stops, 1); expect(service.discarded, ['late']);
    await service.events.close();
  });
  testWidgets('stop confirmation cannot stop an already finished batch', (tester) async {
    final service = StopRaceFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files')); await tester.pumpAndSettle();
    await tester.tap(find.text('Convert 1 Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Stop')); await tester.pumpAndSettle();
    service.updates.add(const BatchUpdate([BatchItem(PhotoItem(id: '1', name: 'one.jpg'), ItemStatus.failed)], finished: true));
    await tester.pump();
    await tester.tap(find.text('Stop Converting')); await tester.pumpAndSettle();
    expect(service.stops, 0);
    await service.updates.close();
  });
  testWidgets('all unavailable items offer file selection', (tester) async {
    await tester.pumpWidget(MomentStaysApp(service: UnavailablePickFake()));
    await tester.tap(find.text('Android → Live Photo')); await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files')); await tester.pumpAndSettle();
    expect(find.text('Not usable'), findsOneWidget);
    expect(find.text('Choose Files'), findsOneWidget);
  });
  testWidgets('global conversion error retains retryable results', (tester) async {
    await tester.pumpWidget(MomentStaysApp(service: GlobalErrorFake()));
    await tester.tap(find.text('Android → Live Photo'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Convert 1 Photo'));
    await tester.pumpAndSettle();
    expect(find.text("Couldn't Convert These Photos"), findsOneWidget);
    expect(find.text('Retry 1 Failed Photo'), findsOneWidget);
  });
  testWidgets('retry saves only newly converted outputs', (tester) async {
    final service = RetrySaveFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Convert 2 Photos'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Save 1 to Photos'));
    await tester.tap(find.text('Save 1 to Photos'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Retry 1 Failed Photo'));
    await tester.tap(find.text('Retry 1 Failed Photo'));
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    await tester.ensureVisible(find.text('Save 1 to Photos'));
    await tester.tap(find.text('Save 1 to Photos'));
    await tester.pumpAndSettle();
    expect(service.saves, [['out-1'], ['out-2']]);
    expect(find.text('Saved to Photos'), findsOneWidget);
  });
  testWidgets('back releases every selection input', (tester) async {
    final service = SuccessFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Back'));
    await tester.pumpAndSettle();
    expect(service.discarded, ['1', '2']);
    expect(find.text('Bring moving photos between phones.'), findsOneWidget);
  });
  testWidgets('Live Photo result cannot be shared and cannot leave during save', (tester) async {
    final service = SuccessFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Convert 2 Photos'));
    await tester.pumpAndSettle();
    expect(find.byTooltip('Share'), findsNothing);
    expect(find.text('Share'), findsNothing);
    await tester.tap(find.text('Save 2 to Photos'));
    await tester.pump();
    final done = tester.widget<CupertinoButton>(find.descendant(of: find.byType(CupertinoNavigationBar), matching: find.widgetWithText(CupertinoButton, 'Done')));
    expect(done.onPressed, isNull);
    await tester.binding.handlePopRoute();
    await tester.pump();
    expect(service.resultDiscards, 0);
    expect(find.text('Conversion Results'), findsOneWidget);
    service.saved.complete();
    await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byType(CupertinoNavigationBar), matching: find.widgetWithText(CupertinoButton, 'Done')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Leave'));
    await tester.pumpAndSettle();
    expect(service.resultDiscards, 1);
  });
  testWidgets('scan reports progress and keeps checked results after cancellation', (tester) async {
    final service = ScanFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Android → Live Photo'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Scan Library'));
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    expect(find.text('Checking photos'), findsOneWidget);
    service.progress.add(const ScanProgress(3, 10, 1, 2));
    await tester.pump();
    expect(find.text('Checked 3 of 10 photos'), findsOneWidget);
    expect(find.text('2 photos could not be checked'), findsOneWidget);
    await tester.tap(find.text('Cancel Scan'));
    await tester.pump();
    expect(service.stopped, isTrue);
    service.result.complete(const [PhotoItem(id: '1', name: 'found.jpg')]);
    await tester.pumpAndSettle();
    expect(find.text('Scan stopped; results are incomplete.'), findsOneWidget);
    expect(find.text('0 of 1 selected'), findsOneWidget);
    expect(find.text('found.jpg'), findsOneWidget);
    await service.progress.close();
  });
  testWidgets('selection excludes unavailable items and shows real failure', (tester) async {
    await tester.pumpWidget(MomentStaysApp(service: FakeService()));
    await tester.tap(find.text('Android → Live Photo'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Choose Files'));
    await tester.pumpAndSettle();
    expect(find.text('1 of 2 selected'), findsOneWidget);
    await tester.tap(find.text('Convert 1 Photo'));
    await tester.pumpAndSettle();
    expect(find.text("Couldn't Convert These Photos"), findsOneWidget);
    expect(find.text('Failed\nFailed'), findsOneWidget);
  });
}
