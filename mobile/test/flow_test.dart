import 'dart:async';
import 'package:flutter/material.dart';
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
void main() {
  testWidgets('retry saves only newly converted outputs', (tester) async {
    final service = RetrySaveFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Choose Files').first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Convert 2 Photos'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Save All 1 to Photos'));
    await tester.tap(find.text('Save All 1 to Photos'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Retry 1 Failed Photo'));
    await tester.tap(find.text('Retry 1 Failed Photo'));
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Save Remaining 1 to Photos'));
    await tester.tap(find.text('Save Remaining 1 to Photos'));
    await tester.pumpAndSettle();
    expect(service.saves, [['out-1'], ['out-2']]);
    expect(find.text('Saved to Photos'), findsOneWidget);
  });
  testWidgets('back releases every selection input', (tester) async {
    final service = SuccessFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Choose Files').first);
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Back'));
    await tester.pumpAndSettle();
    expect(service.discarded, ['1', '2']);
    expect(find.text('Keep every moment moving'), findsOneWidget);
  });
  testWidgets('Live Photo result cannot be shared and cannot leave during save', (tester) async {
    final service = SuccessFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Choose Files').first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Convert 2 Photos'));
    await tester.pumpAndSettle();
    expect(find.byTooltip('Share'), findsNothing);
    expect(find.text('Share'), findsNothing);
    await tester.tap(find.text('Save All 2 to Photos'));
    await tester.pump();
    final done = tester.widget<TextButton>(find.widgetWithText(TextButton, 'Done'));
    expect(done.onPressed, isNull);
    await tester.binding.handlePopRoute();
    await tester.pump();
    expect(service.resultDiscards, 0);
    expect(find.text('Conversion Results'), findsOneWidget);
    service.saved.complete();
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, 'Done'));
    await tester.pumpAndSettle();
    expect(service.resultDiscards, 1);
  });
  testWidgets('scan reports progress and keeps checked results after cancellation', (tester) async {
    final service = ScanFake();
    await tester.pumpWidget(MomentStaysApp(service: service));
    await tester.tap(find.text('Scan Library'));
    await tester.pump();
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
    expect(find.text('Scan stopped.'), findsOneWidget);
    expect(find.text('0 of 1 selected'), findsOneWidget);
    expect(find.text('found.jpg'), findsOneWidget);
    await service.progress.close();
  });
  testWidgets('selection excludes unavailable items and shows real failure', (tester) async {
    await tester.pumpWidget(MomentStaysApp(service: FakeService()));
    await tester.tap(find.text('Choose Files').first);
    await tester.pumpAndSettle();
    expect(find.text('1 of 2 selected'), findsOneWidget);
    await tester.tap(find.text('Convert 1 Photo'));
    await tester.pumpAndSettle();
    expect(find.text("Couldn't Convert These Photos"), findsOneWidget);
    expect(find.text('Failed\nFailed'), findsOneWidget);
  });
}
