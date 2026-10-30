import 'dart:async';
import 'package:flutter/cupertino.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'service.dart';
import 'native_service.dart';
import 'platform_ui.dart';

void main() => runApp(MomentStaysApp(service: NativeService()));

class MomentStaysApp extends StatelessWidget {
  const MomentStaysApp({super.key, this.service = const _DefaultService()});
  final MobileService service;
  @override Widget build(BuildContext context) => CupertinoApp(
    debugShowCheckedModeBanner: false,
    supportedLocales: const [Locale('en'), Locale('zh')],
    localizationsDelegates: const [GlobalCupertinoLocalizations.delegate, GlobalWidgetsLocalizations.delegate],
    theme: const CupertinoThemeData(primaryColor: Color(0xffa65300)),
    home: FlowScreen(service: service),
  );
}
class _DefaultService extends UnavailableService { const _DefaultService(); }

enum PageStep { home, selection, progress, results }
class FlowScreen extends StatefulWidget {
  const FlowScreen({super.key, required this.service});
  final MobileService service;
  @override State<FlowScreen> createState() => _FlowScreenState();
}
class _FlowScreenState extends State<FlowScreen> with WidgetsBindingObserver {
  PageStep page = PageStep.home;
  Direction direction = Direction.motionToLive;
  List<PhotoItem> photos = [];
  Set<String> selected = {};
  List<BatchItem> batch = [];
  String brand = 'Google', filter = 'all';
  bool handled = false, busy = false, stopping = false, resultsReleased = false;
  Source source = Source.photos;
  final Set<String> savedOutputIds = {};
  StreamSubscription<BatchUpdate>? subscription;
  StreamSubscription<ScanProgress>? scanSubscription;
  ScanProgress? scanProgress;
  bool scanning = false, scanStopped = false, scanFailed = false, scanCancelling = false, scanPage = false;
  int scanGeneration = 0, inputGeneration = 0, batchGeneration = 0;
  bool leaveAfterScan = false;
  final Set<Completer<void>> pendingOperations = {};
  Completer<void>? batchClosed;
  bool recoveryPending = false;
  List<PhotoItem> recoveryInputs = [];
  List<BatchItem> recoveryRetained = [];
  bool disposing = false;
  Future<void> tracked(Future<void> Function() operation) async {
    if (disposing) return;
    final token = Completer<void>();
    pendingOperations.add(token);
    try { await operation(); } finally { pendingOperations.remove(token); token.complete(); }
  }
  Future<void> cleanupAfterDispose() async {
    // A stop acknowledgement is only a request; wait for the finished stream
    // and any native picker/save/share operation before releasing its files.
    if (page == PageStep.progress || scanning) {
      try { await widget.service.stop(); } catch (_) { /* wait for completion */ }
    }
    await Future.wait(pendingOperations.map((token) => token.future).toList());
    final closed = batchClosed;
    if (closed != null) await closed.future;
    if (recoveryPending) {
      try { final update = await widget.service.recoverConversion(recoveryInputs); batch = [...recoveryRetained, ...update.items]; recoveryPending = false; }
      catch (_) { return; } // Keep native-owned resources; completion is unconfirmed.
    }
    try {
      if (batch.isNotEmpty && !resultsReleased) await widget.service.discard(batch);
      final used = batch.map((e) => e.photo.id).toSet();
      final unused = photos.where((e) => !used.contains(e.id)).toList();
      if (unused.isNotEmpty) await widget.service.discardInputs(unused);
    } catch (_) { /* bridge retains resources if it is not safe to discard */ }
  }
  bool get zh => Localizations.localeOf(context).languageCode == 'zh';
  String t(String cn, String en) => zh ? cn : en;
  String plural(int n, String singular, [String? plural]) => '$n ${n == 1 ? singular : plural ?? '${singular}s'}';
  List<BatchItem> get successes => batch.where((e) => e.status == ItemStatus.success).toList();
  List<BatchItem> get unsavedSuccesses => successes.where((e) => e.outputId == null || !savedOutputIds.contains(e.outputId)).toList();
  List<BatchItem> get failures => batch.where((e) => e.status == ItemStatus.failed).toList();
  int get processed => batch.where((e) => e.status == ItemStatus.success || e.status == ItemStatus.failed).length;
  @override void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    WidgetsBinding.instance.addPostFrameCallback((_) => checkPendingShare());
  }
  @override void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) checkPendingShare();
  }
  Future<void> checkPendingShare() => tracked(() => _checkPendingShare());
  Future<void> _checkPendingShare() async {
    if (!mounted || page != PageStep.home || busy) return;
    setState(() => busy = true);
    try {
      final shared = await widget.service.takePendingShare();
      if (!mounted) {
        if (shared != null) await widget.service.discardInputs([shared.photo]);
        return;
      }
      if (shared != null) setState(() {
        direction = shared.direction; photos = [shared.photo]; selected = {shared.photo.id};
        scanPage = false; page = PageStep.selection;
      });
    } catch (e) { await error(e); }
    finally { if (mounted) setState(() => busy = false); }
  }
  @override void dispose() {
    disposing = true; inputGeneration++; scanGeneration++;
    WidgetsBinding.instance.removeObserver(this);
    // Keep conversion updates until completion: stop() is not a completion ack.
    scanSubscription?.cancel();
    unawaited(cleanupAfterDispose());
    super.dispose();
  }
  Future<void> error(Object e) async {
    if (!mounted) return;
    await showCupertinoDialog<void>(context: context, builder: (c) => CupertinoAlertDialog(
      title: Text(t('操作未完成', 'Operation Could Not Be Completed')),
      content: Text(e is ServiceUnavailable ? t('此功能尚未连接到设备服务。', 'This feature is not connected to the device service yet.') : t('操作未完成，请重试。', 'The operation could not be completed. Please try again.')),
      actions: [CupertinoDialogAction(onPressed: () => Navigator.pop(c), child: Text(t('好', 'OK')))]));
  }
  Future<void> discardLateInputs(List<PhotoItem> items) async {
    if (items.isEmpty) return;
    try { await widget.service.discardInputs(items); } catch (_) { /* best effort after the view has gone away */ }
  }
  Future<void> pick(Direction d, Source source) => tracked(() => _pick(d, source));
  Future<void> _pick(Direction d, Source source) async {
    if (busy || scanning) return;
    if (source == Source.scan) { await scan(d); return; }
    final generation = ++inputGeneration;
    setState(() => busy = true);
    List<PhotoItem>? pending;
    try {
      pending = await widget.service.pick(d, source);
      if (!mounted || generation != inputGeneration) { await discardLateInputs(pending); return; }
      if (pending.isEmpty) return;
      if (page == PageStep.selection && photos.isNotEmpty) {
        await widget.service.discardInputs(photos);
        photos = []; selected = {};
        if (!mounted || generation != inputGeneration) { await discardLateInputs(pending); return; }
      }
      final items = pending;
      setState(() { direction = d; this.source = source; photos = items; selected = items.where((e) => e.unavailableReason == null).map((e) => e.id).toSet(); scanPage = false; page = PageStep.selection; });
      pending = null;
    } catch (e) {
      if (pending != null) await discardLateInputs(pending);
      await error(e);
    } finally { if (mounted && generation == inputGeneration) setState(() => busy = false); }
  }
  Future<void> scan(Direction d) => tracked(() => _scan(d));
  Future<void> _scan(Direction d) async {
    if (busy || scanning) return;
    final generation = ++scanGeneration;
    final inputToken = ++inputGeneration;
    setState(() => busy = true);
    StreamSubscription<ScanProgress>? ownSubscription;
    try {
      if (photos.isNotEmpty && page == PageStep.selection) {
        await widget.service.discardInputs(photos);
        photos = []; selected = {};
        if (!mounted || inputToken != inputGeneration) return;
        setState(() { photos = []; selected = {}; });
      }
      final previous = scanSubscription;
      if (previous != null) await previous.cancel();
      if (!mounted || generation != scanGeneration || inputToken != inputGeneration) return;
      setState(() { direction = d; source = Source.scan; photos = []; selected = {}; scanProgress = null; scanning = true; scanStopped = false; scanFailed = false; scanCancelling = false; scanPage = true; page = PageStep.selection; busy = false; });
      ownSubscription = widget.service.scanProgress.listen((value) {
        if (mounted && generation == scanGeneration) setState(() => scanProgress = value);
      }, onError: (Object e) {
        if (mounted && generation == scanGeneration) setState(() => scanFailed = true);
      });
      scanSubscription = ownSubscription;
      final items = await widget.service.pick(d, Source.scan);
      if (!mounted || generation != scanGeneration || inputToken != inputGeneration) { await discardLateInputs(items); return; }
      setState(() { photos = items; selected = {}; scanning = false; scanCancelling = false; });
      if (leaveAfterScan) { leaveAfterScan = false; await backFromSelection(); }
    } catch (e) {
      if (!mounted || generation != scanGeneration || inputToken != inputGeneration) return;
      setState(() { scanning = false; scanCancelling = false; scanFailed = !scanStopped; busy = false; });
      if (leaveAfterScan) { leaveAfterScan = false; await backFromSelection(); } else if (!scanStopped) { await error(e); }
    } finally {
      if (identical(scanSubscription, ownSubscription)) scanSubscription = null;
      if (ownSubscription != null) await ownSubscription.cancel();
    }
  }
  Future<void> cancelScan({bool leave = false}) async {
    if (!scanning || scanCancelling) return;
    final generation = scanGeneration;
    setState(() { scanCancelling = true; scanStopped = true; leaveAfterScan = leave; });
    try {
      await widget.service.stop();
    } catch (e) {
      if (!mounted || generation != scanGeneration || !scanning) return;
      setState(() { scanCancelling = false; scanStopped = false; leaveAfterScan = false; });
      await error(e);
    }
  }
  Future<void> backFromSelection() => tracked(() => _backFromSelection());
  Future<void> _backFromSelection() async {
    if (busy) return;
    if (scanning) { await cancelScan(leave: true); return; }
    setState(() => busy = true);
    try {
      if (photos.isNotEmpty) await widget.service.discardInputs(photos);
      photos = []; selected = {};
      if (mounted) setState(() { page = PageStep.home; });
    } catch (e) { await error(e); } finally { if (mounted) { setState(() => busy = false); if (page == PageStep.home) checkPendingShare(); } }
  }
  Future<void> start(List<PhotoItem> items, {List<BatchItem> retained = const []}) => tracked(() => _start(items, retained: retained));
  Future<void> _start(List<PhotoItem> items, {List<BatchItem> retained = const []}) async {
    if (items.isEmpty || busy || scanning) return;
    final generation = ++batchGeneration;
    setState(() => busy = true);
    try {
      final previous = subscription;
      if (previous != null) { await previous.cancel(); final oldClosed = batchClosed; if (oldClosed != null && !oldClosed.isCompleted) oldClosed.complete(); }
      if (!mounted || generation != batchGeneration) return;
      setState(() { batch = [...retained, ...items.map((e) => BatchItem(e, ItemStatus.waiting))]; page = PageStep.progress; stopping = false; resultsReleased = false; if (retained.isEmpty) savedOutputIds.clear(); handled = false; busy = false; });
      recoveryPending = false; recoveryInputs = List.of(items); recoveryRetained = List.of(retained);
      final closed = Completer<void>();
      batchClosed = closed;
      subscription = widget.service.convert(direction, items, brand).listen((update) {
        if (generation != batchGeneration) return;
        if (!mounted) { batch = [...retained, ...update.items]; return; }
        setState(() { batch = [...retained, ...update.items]; if (update.finished) { page = PageStep.results; stopping = false; } });
      }, onError: (Object e) { if (generation != batchGeneration) return; if (e is RecoveryUnconfirmed) { recoveryPending = true; if (mounted) setState(() { page = PageStep.progress; stopping = false; }); return; } if (!mounted) return; setState(() { batch = batch.map((item) => item.status == ItemStatus.waiting || item.status == ItemStatus.converting ? BatchItem(item.photo, ItemStatus.failed, reason: t('转换未完成，请重试。', 'Conversion could not be completed. Please try again.')) : item).toList(); page = PageStep.results; stopping = false; }); }, onDone: () { if (!closed.isCompleted) closed.complete(); if (generation == batchGeneration) subscription = null; });
    } catch (e) { if (mounted && generation == batchGeneration) { setState(() => busy = false); await error(e); } }
  }
  Future<void> retryRecovery() => tracked(() async {
    if (!recoveryPending || busy) return;
    setState(() => busy = true);
    try {
      final update = await widget.service.recoverConversion(recoveryInputs);
      if (!mounted) return;
      setState(() { batch = [...recoveryRetained, ...update.items]; recoveryPending = false; page = PageStep.results; });
    } catch (e) { await error(e); } finally { if (mounted) setState(() => busy = false); }
  });
  Future<bool> confirm(String title, String message, String action, {String? cancel}) => platformConfirm(context, title: title, message: message, action: action, cancel: cancel ?? t('留下', 'Stay'));
  Future<void> stop() async {
    if (stopping || page != PageStep.progress) return;
    final generation = batchGeneration;
    if (await confirm(t('停止转换？', 'Stop converting?'), t('已完成的照片会保留，尚未处理的照片不会转换。', "Completed photos will be kept. Photos not yet processed won't be converted."), t('停止转换', 'Stop Converting'), cancel: t('继续转换', 'Continue Converting'))) {
      if (!mounted || page != PageStep.progress || generation != batchGeneration || stopping) return;
      setState(() => stopping = true);
      try { await widget.service.stop(); } catch (e) { if (mounted) setState(() => stopping = false); await error(e); }
    }
  }
  Future<void> perform(Future<void> Function() action, {bool markHandled = false, List<BatchItem>? savedItems}) => tracked(() => _perform(action, markHandled: markHandled, savedItems: savedItems));
  Future<void> _perform(Future<void> Function() action, {bool markHandled = false, List<BatchItem>? savedItems}) async {
    if (busy) return;
    setState(() => busy = true);
    try { await action(); if (mounted) setState(() { if (markHandled) handled = true; if (savedItems != null) savedOutputIds.addAll(savedItems.map((e) => e.outputId).whereType<String>()); }); } catch (e) { if (savedItems != null && mounted) { await showCupertinoDialog<void>(context: context, builder: (c) => CupertinoAlertDialog(title: Text(t('无法确认保存结果', "Couldn't confirm the save result")), content: Text(t('无法确认保存结果，部分照片可能已存入“照片”。请先在“照片”中检查，再决定是否重试。', "Couldn't confirm the save result. Some photos may have been saved to Photos. Check Photos before trying again.")), actions: [CupertinoDialogAction(onPressed: () => Navigator.pop(c), child: Text(t('好', 'OK')))])); } else { await error(e); } } finally { if (mounted) setState(() => busy = false); }
  }
  Future<void> done() => tracked(() => _done());
  Future<void> _done() async {
    if (busy) return;
    if (!resultsReleased && successes.isNotEmpty && !await confirm(t('离开转换结果？', 'Leave Conversion Results?'), t('离开后，未分享或未存入的转换结果将被清除。', "Converted results that haven't been shared or saved will be cleared when you leave."), t('离开', 'Leave'))) return;
    if (!mounted || busy) return;
    setState(() => busy = true);
    try {
      final used = batch.map((e) => e.photo.id).toSet();
      if (!resultsReleased) {
        await widget.service.discard(batch);
        resultsReleased = true;
        photos = photos.where((e) => !used.contains(e.id)).toList();
      }
      final unused = photos.where((e) => !used.contains(e.id)).toList();
      if (unused.isNotEmpty) await widget.service.discardInputs(unused);
      photos = []; batch = []; selected = {}; savedOutputIds.clear(); resultsReleased = false;
      if (mounted) setState(() { page = PageStep.home; });
    } catch (e) { await error(e); } finally { if (mounted) { setState(() => busy = false); if (page == PageStep.home) checkPendingShare(); } }
  }
  static const accent = Color(0xfff0a43c);
  Widget action(String text, VoidCallback? onPressed) => PlatformAction(label: text, onPressed: onPressed, busy: busy);
  Widget group(Widget child) => Container(decoration: BoxDecoration(color: CupertinoDynamicColor.resolve(CupertinoColors.secondarySystemGroupedBackground, context), borderRadius: BorderRadius.circular(12)), child: child);
  Widget line(String title, {String? subtitle, Widget? leading, Widget? trailing, VoidCallback? onTap}) {
    final content = ConstrainedBox(constraints: const BoxConstraints(minHeight: 56), child: Padding(padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12), child: Row(children: [if (leading != null) ...[leading, const SizedBox(width: 12)], Expanded(child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [Text(title, style: TextStyle(fontSize: 17, color: CupertinoDynamicColor.resolve(CupertinoColors.label, context))), if (subtitle != null) Text(subtitle, style: TextStyle(fontSize: 15, color: CupertinoDynamicColor.resolve(CupertinoColors.secondaryLabel, context)))])), if (trailing != null) trailing])));
    return onTap == null ? content : CupertinoButton(padding: EdgeInsets.zero, onPressed: onTap, child: content);
  }
  Future<void> sources(Direction d) async => showCupertinoModalPopup<void>(context: context, builder: (c) => CupertinoActionSheet(title: Text(d == Direction.motionToLive ? t('安卓 → 实况照片', 'Android → Live Photo') : t('实况照片 → 安卓', 'Live Photo → Android')), actions: [for (final entry in [(Source.photos, t('从照片中选择', 'Choose Photos')), (Source.files, t('从文件中选择', 'Choose Files')), if (d == Direction.motionToLive) (Source.scan, t('扫描相册', 'Scan Library'))]) CupertinoActionSheetAction(onPressed: () { Navigator.pop(c); pick(d, entry.$1); }, child: Text(entry.$2))], cancelButton: CupertinoActionSheetAction(onPressed: () => Navigator.pop(c), child: Text(t('取消', 'Cancel')))));
  @override Widget build(BuildContext context) => Navigator(pages: [CupertinoPage<void>(key: const ValueKey('home'), child: screen(PageStep.home)), if (page != PageStep.home) CupertinoPage<void>(key: ValueKey(page), child: screen(page))], onPopPage: (route, result) { if (page == PageStep.selection) { backFromSelection(); return false; } return false; });
  Widget screen(PageStep step) {
    final title = switch (step) { PageStep.home => t('留刻', 'MomentStays'), PageStep.selection => scanPage ? t('扫描相册', 'Scan Library') : t('选择照片', 'Select Photos'), PageStep.progress => t('正在转换', 'Converting'), PageStep.results => t('转换结果', 'Conversion Results') };
    return PopScope(canPop: step == PageStep.home, child: CupertinoPageScaffold(backgroundColor: CupertinoColors.systemGroupedBackground, navigationBar: step == PageStep.home ? null : CupertinoNavigationBar(automaticallyImplyLeading: false, middle: Text(title), leading: step == PageStep.selection ? CupertinoButton(padding: EdgeInsets.zero, onPressed: busy ? null : backFromSelection, child: Text(t('返回', 'Back'))) : step == PageStep.progress ? CupertinoButton(padding: EdgeInsets.zero, onPressed: stopping || recoveryPending ? null : stop, child: Text(stopping ? t('正在停止…', 'Stopping…') : t('停止', 'Stop'))) : null, trailing: step == PageStep.results ? CupertinoButton(padding: EdgeInsets.zero, onPressed: busy ? null : done, child: Text(t('完成', 'Done'))) : null), child: SafeArea(child: switch (step) { PageStep.home => home(title), PageStep.selection => selection(), PageStep.progress => progress(), PageStep.results => results() })));
  }
  Widget home(String title) => CustomScrollView(slivers: [CupertinoSliverNavigationBar(largeTitle: Text(title)), SliverToBoxAdapter(child: Padding(padding: const EdgeInsets.all(20), child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [Text(t('让会动的照片，在两种手机间流转。', 'Bring moving photos between phones.'), style: TextStyle(color: CupertinoDynamicColor.resolve(CupertinoColors.secondaryLabel, context))), const SizedBox(height: 24), Text(t('转换', 'Convert')), const SizedBox(height: 8), group(Column(children: [line(t('安卓 → 实况照片', 'Android → Live Photo'), subtitle: t('把动态照片变成实况照片。', 'Turn a Motion Photo into a Live Photo.'), leading: const Icon(CupertinoIcons.photo_on_rectangle), trailing: const Icon(CupertinoIcons.chevron_forward), onTap: busy ? null : () => sources(Direction.motionToLive)), line(t('实况照片 → 安卓', 'Live Photo → Android'), subtitle: t('转换实况照片，以文件形式分享。', 'Convert a Live Photo to share as a file.'), leading: const Icon(CupertinoIcons.device_phone_portrait), trailing: const Icon(CupertinoIcons.chevron_forward), onTap: busy ? null : () => sources(Direction.liveToMotion))])), const SizedBox(height: 16), Text(t('照片只在本机处理，不会上传。\n也可从照片或文件分享至留刻。', 'Photos are processed on this device and never uploaded.\nYou can also share photos or files to MomentStays.'), style: TextStyle(fontSize: 13, color: CupertinoDynamicColor.resolve(CupertinoColors.secondaryLabel, context)))])))]);
  Widget selection() {
    final available = photos.where((e) => e.unavailableReason == null).toList();
    final all = available.isNotEmpty && available.every((e) => selected.contains(e.id));
    return Column(children: [Expanded(child: ListView(padding: const EdgeInsets.all(20), children: [if (scanPage) scanCard(), Row(children: [Expanded(child: Text(t('已选 ${selected.length} / ${photos.length} 张', '${selected.length} of ${photos.length} selected'), style: TextStyle(color: CupertinoDynamicColor.resolve(CupertinoColors.secondaryLabel, context)))), CupertinoButton(onPressed: available.isEmpty ? null : () => setState(() { if (all) { selected.clear(); } else { selected.addAll(available.map((e) => e.id)); } }), child: Text(all ? t('取消全选', 'Deselect All') : t('全选', 'Select All')))]), if (photos.isEmpty && !scanning) ...[Text(scanFailed ? t('扫描未完成', 'Scan could not be completed') : t('没有找到动态照片', 'No Motion Photos Found')), CupertinoButton(onPressed: () => pick(direction, Source.files), child: Text(t('从文件中选择', 'Choose Files')))], if (photos.isNotEmpty) selectionItems(), if (source != Source.files && photos.any((p) => p.unavailableReason != null)) ...[const SizedBox(height: 12), group(Column(children: photos.where((p) => p.unavailableReason != null).map((p) => line(p.name, subtitle: p.unavailableReason, leading: thumb(p))).toList())), CupertinoButton(onPressed: busy ? null : () => pick(direction, Source.files), child: Text(t('从文件中选择', 'Choose Files')))], if (photos.isNotEmpty && available.isEmpty && source == Source.files && !scanning) ...[Text(t('没有可以转换的照片', 'No Photos to Convert')), CupertinoButton(onPressed: busy ? null : () => pick(direction, Source.files), child: Text(t('从文件中选择', 'Choose Files')))], if (direction == Direction.liveToMotion) ...[const SizedBox(height: 24), group(line(t('对方的手机品牌', 'Their phone brand'), trailing: Text('$brand  ›', style: TextStyle(color: CupertinoDynamicColor.resolve(CupertinoColors.secondaryLabel, context))), onTap: chooseBrand))]])), Padding(padding: const EdgeInsets.fromLTRB(20, 12, 20, 12), child: SizedBox(width: double.infinity, child: action(t('转换 ${selected.length} 张', 'Convert ${plural(selected.length, 'Photo')}'), selected.isEmpty || scanning || busy ? null : () => start(photos.where((e) => selected.contains(e.id)).toList()))))]);
  }
  void toggle(PhotoItem p) { if (p.unavailableReason == null) setState(() { if (!selected.add(p.id)) selected.remove(p.id); }); }
  Widget selectionItems() {
    if (source == Source.files) return group(Column(children: photos.map((p) => Semantics(selected: selected.contains(p.id), enabled: p.unavailableReason == null, child: line(p.name, subtitle: p.unavailableReason, leading: thumb(p), trailing: Icon(selected.contains(p.id) ? CupertinoIcons.check_mark_circled_solid : CupertinoIcons.circle, color: selected.contains(p.id) ? accent : CupertinoColors.systemGrey), onTap: p.unavailableReason == null ? () => toggle(p) : null))).toList()));
    return LayoutBuilder(builder: (context, constraints) {
      final count = MediaQuery.textScalerOf(context).scale(1) >= 1.5 || constraints.maxWidth < 280 ? 2 : constraints.maxWidth >= 720 ? 4 : 3;
      return GridView.builder(shrinkWrap: true, physics: const NeverScrollableScrollPhysics(), itemCount: photos.length, gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(crossAxisCount: count, crossAxisSpacing: 4, mainAxisSpacing: 4), itemBuilder: (context, i) {
        final p = photos[i], chosen = selected.contains(p.id);
        return Semantics(button: true, selected: chosen, enabled: p.unavailableReason == null, label: p.name, value: p.unavailableReason ?? (chosen ? t('已选中', 'Selected') : t('未选中', 'Not selected')), child: GestureDetector(onTap: p.unavailableReason == null ? () => toggle(p) : null, child: ClipRRect(borderRadius: BorderRadius.circular(2), child: Stack(fit: StackFit.expand, children: [if (p.thumbnail != null) Image.memory(p.thumbnail!, fit: BoxFit.cover) else Container(color: CupertinoColors.systemGrey5, child: Column(mainAxisAlignment: MainAxisAlignment.center, children: [const Icon(CupertinoIcons.photo, color: CupertinoColors.systemGrey), Padding(padding: const EdgeInsets.all(4), child: Text(p.name, textAlign: TextAlign.center, maxLines: 2, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 13, color: CupertinoDynamicColor.resolve(CupertinoColors.label, context))))])), Positioned(top: 4, right: 4, child: Icon(chosen ? CupertinoIcons.check_mark_circled_solid : CupertinoIcons.circle, color: chosen ? accent : CupertinoColors.systemGrey)), if (p.unavailableReason != null) Positioned(bottom: 0, left: 0, right: 0, child: Container(color: CupertinoColors.systemBackground, padding: const EdgeInsets.all(4), child: Text(p.unavailableReason!, maxLines: 2, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12))))]))));
      });
    });
  }
  Future<void> chooseBrand() async { final value = await Navigator.of(context).push<String>(CupertinoPageRoute(builder: (c) => CupertinoPageScaffold(navigationBar: CupertinoNavigationBar(middle: Text(t('对方的手机品牌', 'Their phone brand'))), child: SafeArea(child: ListView(children: ['Google', 'Xiaomi', 'Samsung', 'HUAWEI', 'HONOR', 'OPPO', 'vivo'].map((b) => line(b, trailing: b == brand ? const Icon(CupertinoIcons.check_mark) : null, onTap: () => Navigator.pop(c, b))).toList()))))); if (value != null && mounted) setState(() => brand = value); }
  Widget thumb(PhotoItem p) => ClipRRect(borderRadius: BorderRadius.circular(6), child: SizedBox(width: 44, height: 44, child: p.thumbnail == null ? const ColoredBox(color: CupertinoColors.systemGrey5, child: Icon(CupertinoIcons.doc, color: CupertinoColors.systemGrey)) : Image.memory(p.thumbnail!, fit: BoxFit.cover)));
  Widget scanCard() { final p = scanProgress; final msg = scanning ? (p == null || p.total <= 0 ? t('正在检查照片', 'Checking photos') : t('已检查 ${p.completed} / ${p.total} 张', 'Checked ${p.completed} of ${p.total} photos')) : scanFailed ? t('扫描未完成', 'Scan could not be completed') : scanStopped ? t('扫描已停止，结果不完整。', 'Scan stopped; results are incomplete.') : t('扫描完成', 'Scan complete'); return group(Padding(padding: const EdgeInsets.all(16), child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text(t('只检查你允许访问的照片原件。', 'Only photos you allow access to are checked.')), Text(msg), if (scanning && p != null) Text(t('已找到 ${p.matches} 张照片，扫描结束后可选择。', 'Found ${p.matches} photo(s). You can select them when the scan finishes.')), if (scanning) Padding(padding: const EdgeInsets.symmetric(vertical: 8), child: PlatformProgress(value: p == null || p.total <= 0 ? null : p.completed / p.total, label: msg)), if (p != null && p.issues > 0) Text(t('${p.issues} 张无法检查', '${p.issues} photos could not be checked')), CupertinoButton(padding: EdgeInsets.zero, onPressed: scanning ? (scanCancelling ? null : () => cancelScan()) : () => scan(direction), child: Text(scanning ? (scanCancelling ? t('正在停止…', 'Stopping…') : t('取消扫描', 'Cancel Scan')) : t('重新扫描', 'Scan Again')))]))); }
  Widget batchList() => group(Column(children: batch.where((e) => filter == 'all' || (filter == 'success' && e.status == ItemStatus.success) || (filter == 'failed' && e.status == ItemStatus.failed)).map((e) { final (icon, status) = switch (e.status) { ItemStatus.waiting => (CupertinoIcons.clock, t('等待', 'Waiting')), ItemStatus.converting => (CupertinoIcons.clock, t('转换中', 'Converting')), ItemStatus.success => (CupertinoIcons.check_mark_circled_solid, t('完成', 'Done')), ItemStatus.failed => (CupertinoIcons.exclamationmark_circle, t('失败', 'Failed')), ItemStatus.skipped => (CupertinoIcons.minus_circle, t('未转换', 'Not Converted')) }; return line(e.photo.name, leading: thumb(e.photo), subtitle: e.reason == null ? status : '$status\n${e.reason}', trailing: e.status == ItemStatus.converting ? const CupertinoActivityIndicator() : Icon(icon), onTap: null); }).toList()));
  Widget progress() { if (recoveryPending) return Center(child: Padding(padding: const EdgeInsets.all(20), child: Column(mainAxisSize: MainAxisSize.min, children: [Text(t('无法确认转换结果。请重试恢复，照片和结果仍保留在本机。', 'Could not confirm the conversion result. Retry recovery; the inputs and results remain on this device.')), CupertinoButton(onPressed: busy ? null : retryRecovery, child: Text(t('重试恢复', 'Retry Recovery')))]))); final current = batch.where((e) => e.status == ItemStatus.converting).toList(); final label = t('已处理 $processed / ${batch.length} 张', 'Processed $processed of ${batch.length} photos'); return ListView(padding: const EdgeInsets.all(20), children: [Text(label, style: const TextStyle(fontSize: 22, fontWeight: FontWeight.w600)), if (current.isNotEmpty) Padding(padding: const EdgeInsets.symmetric(vertical: 8), child: Text(current.first.photo.name, style: TextStyle(color: CupertinoDynamicColor.resolve(CupertinoColors.secondaryLabel, context)))), Padding(padding: const EdgeInsets.symmetric(vertical: 16), child: PlatformProgress(value: batch.isEmpty ? null : processed / batch.length, label: label)), batchList()]); }
  Widget results() { final ok = successes.length, failed = failures.length, unsaved = unsavedSuccesses; if (resultsReleased) return Center(child: Padding(padding: const EdgeInsets.all(20), child: Column(mainAxisSize: MainAxisSize.min, children: [Text(t('结果已清理，剩余输入尚未清理。请点完成重试。', 'Results were cleared, but some inputs remain. Tap Done to retry cleanup.')), CupertinoButton(onPressed: busy ? null : done, child: Text(t('完成', 'Done')))]))); void retry() { filter = 'all'; start(failures.map((e) => e.photo).toList(), retained: batch.where((e) => e.status != ItemStatus.failed).toList()); } return Column(children: [Expanded(child: ListView(padding: const EdgeInsets.all(20), children: [Icon(ok == 0 ? CupertinoIcons.exclamationmark_circle : CupertinoIcons.check_mark_circled_solid, size: 56, color: ok == 0 ? CupertinoColors.systemRed : CupertinoColors.systemGreen), const SizedBox(height: 12), Text(ok == 0 ? t('没能转换这些照片', "Couldn't Convert These Photos") : t('$ok 张已转换，$failed 张失败', '$ok converted, $failed failed'), textAlign: TextAlign.center, style: const TextStyle(fontSize: 22, fontWeight: FontWeight.w600)), if (ok > 0 && failed > 0) Padding(padding: const EdgeInsets.symmetric(vertical: 16), child: CupertinoSlidingSegmentedControl<String>(groupValue: filter, children: {'all': Text(t('全部', 'All')), 'success': Text(t('成功', 'Converted')), 'failed': Text(t('失败', 'Failed'))}, onValueChanged: (v) { if (v != null) setState(() => filter = v); })), const SizedBox(height: 16), batchList(), if (ok > 0 && failed > 0) group(line(t('重试失败的 $failed 张', 'Retry ${plural(failed, 'Failed Photo')}'), onTap: busy ? null : retry)), if (ok > 0 && direction == Direction.liveToMotion) group(line(t('存到“文件”', 'Save to Files'), onTap: busy ? null : () => perform(() => widget.service.saveToFiles(successes)))), if (ok > 0 && direction == Direction.motionToLive) Text(t('暂不支持直接分享实况照片。请先存入照片，再从“照片”分享。', "Direct sharing of Live Photos isn't available yet. Save them to Photos, then share from there."))])), if (ok > 0 || failed > 0) Padding(padding: const EdgeInsets.fromLTRB(20, 12, 20, 12), child: SizedBox(width: double.infinity, child: action(busy ? (direction == Direction.motionToLive ? t('正在存入…', 'Saving…') : t('正在导出…', 'Exporting…')) : ok == 0 ? t('重试失败的 $failed 张', 'Retry ${plural(failed, 'Failed Photo')}') : direction == Direction.liveToMotion ? t('分享 $ok 个文件', 'Share ${plural(ok, 'File')}') : unsaved.isEmpty ? t('已存入照片', 'Saved to Photos') : t('存入照片（${unsaved.length}）', 'Save ${unsaved.length} to Photos'), busy ? null : ok == 0 ? retry : direction == Direction.liveToMotion ? () => perform(() => widget.service.share(successes)) : unsaved.isEmpty ? null : () => perform(() => widget.service.saveToPhotos(unsaved), savedItems: unsaved))))]); }
}
