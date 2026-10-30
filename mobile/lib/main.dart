import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'service.dart';
import 'native_service.dart';

void main() => runApp(MomentStaysApp(service: NativeService()));

class MomentStaysApp extends StatelessWidget {
  const MomentStaysApp({super.key, this.service = const _DefaultService()});
  final MobileService service;
  @override Widget build(BuildContext context) => MaterialApp(
    debugShowCheckedModeBanner: false,
    supportedLocales: const [Locale('en'), Locale('zh')],
    localizationsDelegates: GlobalMaterialLocalizations.delegates,
    theme: ThemeData(colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xffe47d37), surface: const Color(0xfffffaf5)), useMaterial3: true),
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
  bool handled = false, busy = false;
  final Set<String> savedOutputIds = {};
  StreamSubscription<BatchUpdate>? subscription;
  StreamSubscription<ScanProgress>? scanSubscription;
  ScanProgress? scanProgress;
  bool scanning = false, scanStopped = false, scanFailed = false, scanCancelling = false, scanPage = false;
  int scanGeneration = 0;
  bool leaveAfterScan = false;
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
  Future<void> checkPendingShare() async {
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
  @override void dispose() { WidgetsBinding.instance.removeObserver(this); subscription?.cancel(); scanSubscription?.cancel(); if (scanning) { widget.service.stop().catchError((_) {}); } super.dispose(); }
  Future<void> error(Object e) async {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e is ServiceUnavailable ? t('此功能尚未连接到设备服务。', 'This feature is not connected to the device service yet.') : t('操作未完成，请重试。', 'The operation could not be completed. Please try again.'))));
  }
  Future<void> pick(Direction d, Source source) async {
    if (busy) return;
    if (source == Source.scan) {
      await scan(d);
      return;
    }
    try {
      final items = await widget.service.pick(d, source);
      if (!mounted || items.isEmpty) return;
      if (page == PageStep.selection && photos.isNotEmpty) {
        try { await widget.service.discardInputs(photos); } catch (e) {
          await widget.service.discardInputs(items);
          rethrow;
        }
      }
      if (!mounted) return;
      setState(() { direction = d; photos = items; selected = items.where((e) => e.unavailableReason == null).map((e) => e.id).toSet(); scanPage = false; page = PageStep.selection; });
    } catch (e) { await error(e); }
  }
  Future<void> scan(Direction d) async {
    if (busy || scanning) return;
    if (photos.isNotEmpty && page == PageStep.selection) {
      setState(() => busy = true);
      try { await widget.service.discardInputs(photos); } catch (e) { if (mounted) setState(() => busy = false); await error(e); return; }
      if (!mounted) return;
      setState(() { photos = []; selected = {}; busy = false; });
    }
    final generation = ++scanGeneration;
    await scanSubscription?.cancel();
    if (!mounted || generation != scanGeneration) return;
    setState(() { direction = d; photos = []; selected = {}; scanProgress = null; scanning = true; scanStopped = false; scanFailed = false; scanCancelling = false; scanPage = true; page = PageStep.selection; });
    scanSubscription = widget.service.scanProgress.listen((value) {
      if (mounted && generation == scanGeneration) setState(() => scanProgress = value);
    }, onError: (Object e) {
      if (mounted && generation == scanGeneration) setState(() => scanFailed = true);
    });
    try {
      final items = await widget.service.pick(d, Source.scan);
      if (!mounted || generation != scanGeneration) return;
      setState(() { photos = items; selected = {}; scanning = false; scanCancelling = false; });
      if (leaveAfterScan) { leaveAfterScan = false; await backFromSelection(); }
    } catch (e) {
      if (!mounted || generation != scanGeneration) return;
      setState(() { scanning = false; scanCancelling = false; scanFailed = !scanStopped; });
      if (leaveAfterScan) { leaveAfterScan = false; await backFromSelection(); } else if (!scanStopped) { await error(e); }
    } finally {
      if (generation == scanGeneration) { await scanSubscription?.cancel(); scanSubscription = null; }
    }
  }
  Future<void> cancelScan({bool leave = false}) async {
    if (!scanning || scanCancelling) return;
    setState(() { scanCancelling = true; scanStopped = true; leaveAfterScan = leave; });
    try {
      await widget.service.stop();
    } catch (e) {
      if (mounted) setState(() { scanCancelling = false; scanStopped = false; leaveAfterScan = false; });
      await error(e);
    }
  }
  Future<void> backFromSelection() async {
    if (busy) return;
    if (scanning) { await cancelScan(leave: true); return; }
    setState(() => busy = true);
    try {
      if (photos.isNotEmpty) await widget.service.discardInputs(photos);
      if (mounted) setState(() { photos = []; selected = {}; page = PageStep.home; });
    } catch (e) { await error(e); } finally { if (mounted) { setState(() => busy = false); if (page == PageStep.home) checkPendingShare(); } }
  }
  void start(List<PhotoItem> items, {List<BatchItem> retained = const []}) {
    if (items.isEmpty) return;
    subscription?.cancel();
    setState(() { batch = [...retained, ...items.map((e) => BatchItem(e, ItemStatus.waiting))]; page = PageStep.progress; if (retained.isEmpty) savedOutputIds.clear(); handled = false; });
    subscription = widget.service.convert(direction, items, brand).listen((update) {
      if (!mounted) return;
      setState(() { batch = [...retained, ...update.items]; if (update.finished) page = PageStep.results; });
    }, onError: (Object e) { if (!mounted) return; setState(() { page = PageStep.selection; }); error(e); });
  }
  Future<bool> confirm(String title, String message, String action) async => await showDialog<bool>(context: context, builder: (c) => AlertDialog(title: Text(title), content: Text(message), actions: [TextButton(onPressed: () => Navigator.pop(c, false), child: Text(t('留下', 'Stay'))), TextButton(onPressed: () => Navigator.pop(c, true), child: Text(action))])) ?? false;
  Future<void> stop() async {
    if (await confirm(t('停止转换？', 'Stop converting?'), t('已完成的照片会保留，尚未处理的照片不会转换。', "Completed photos will be kept. Photos not yet processed won't be converted."), t('停止转换', 'Stop Converting'))) {
      try { await widget.service.stop(); } catch (e) { await error(e); }
    }
  }
  Future<void> perform(Future<void> Function() action, {bool markHandled = false, List<BatchItem>? savedItems}) async {
    setState(() => busy = true);
    try { await action(); if (mounted) setState(() { if (markHandled) handled = true; if (savedItems != null) savedOutputIds.addAll(savedItems.map((e) => e.outputId).whereType<String>()); }); } catch (e) { await error(e); } finally { if (mounted) setState(() => busy = false); }
  }
  Future<void> done() async {
    if (busy) return;
    if (unsavedSuccesses.isNotEmpty && !handled && !await confirm(t('离开转换结果？', 'Leave Conversion Results?'), t('离开后，未分享或未存入的转换结果将被清除。', "Converted results that haven't been shared or saved will be cleared when you leave."), t('离开', 'Leave'))) return;
    if (!mounted || busy) return;
    setState(() => busy = true);
    try {
      await widget.service.discard(batch);
      final used = batch.map((e) => e.photo.id).toSet();
      final unused = photos.where((e) => !used.contains(e.id)).toList();
      if (unused.isNotEmpty) await widget.service.discardInputs(unused);
      if (mounted) setState(() { page = PageStep.home; batch = []; photos = []; selected = {}; savedOutputIds.clear(); });
    } catch (e) { await error(e); } finally { if (mounted) { setState(() => busy = false); if (page == PageStep.home) checkPendingShare(); } }
  }
  @override Widget build(BuildContext context) {
    final title = switch (page) { PageStep.home => t('留刻', 'MomentStays'), PageStep.selection => t('选择要转换的照片', 'Choose Photos to Convert'), PageStep.progress => t('正在转换', 'Converting'), PageStep.results => t('转换结果', 'Conversion Results') };
    return PopScope(canPop: page == PageStep.home, onPopInvokedWithResult: (didPop, _) { if (!didPop) { if (page == PageStep.selection) backFromSelection(); if (page == PageStep.progress) stop(); if (page == PageStep.results) done(); } }, child: Scaffold(
      appBar: AppBar(title: Text(title), leading: page == PageStep.home ? null : IconButton(icon: Icon(page == PageStep.progress ? Icons.stop_circle_outlined : Icons.arrow_back), tooltip: page == PageStep.progress ? t('停止', 'Stop') : t('返回', 'Back'), onPressed: busy ? null : () { if (page == PageStep.selection) backFromSelection(); if (page == PageStep.progress) stop(); if (page == PageStep.results) done(); }), actions: [if (page == PageStep.results) TextButton(onPressed: busy ? null : done, child: Text(t('完成', 'Done')))]),
      body: SafeArea(child: LayoutBuilder(builder: (context, constraints) { final wide = constraints.maxWidth >= 760 && MediaQuery.textScalerOf(context).scale(1) < 1.5; return switch (page) { PageStep.home => home(wide), PageStep.selection => selection(wide), PageStep.progress => progress(wide), PageStep.results => results(wide) }; })),
    ));
  }
  Widget home(bool wide) {
    final intro = Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Icon(Icons.motion_photos_on, size: 52, color: Theme.of(context).colorScheme.primary), const SizedBox(height: 20), Text(t('留住每一个瞬间', 'Keep every moment moving'), style: Theme.of(context).textTheme.headlineMedium), const SizedBox(height: 12), Text(t('让会动的照片，在两种手机间流转。', 'Bring moving photos between phones.'), style: Theme.of(context).textTheme.bodyLarge), const SizedBox(height: 16), Text(t('照片只在本机处理，不会上传。', 'Photos are processed on this device and never uploaded.'))]);
    final cards = Column(children: [directionCard(Direction.motionToLive, t('安卓 → 实况照片', 'Android → Live Photo'), t('把动态照片变成实况照片。', 'Turn a Motion Photo into a Live Photo.')), const SizedBox(height: 12), directionCard(Direction.liveToMotion, t('实况照片 → 安卓', 'Live Photo → Android'), t('转换实况照片，以文件形式分享。', 'Convert a Live Photo to share as a file.'))]);
    return SingleChildScrollView(padding: const EdgeInsets.all(20), child: Center(child: ConstrainedBox(constraints: const BoxConstraints(maxWidth: 1000), child: wide ? Row(crossAxisAlignment: CrossAxisAlignment.start, children: [Expanded(flex: 4, child: intro), const SizedBox(width: 24), Expanded(flex: 6, child: cards)]) : Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [intro, const SizedBox(height: 32), cards]))));
  }
  Widget directionCard(Direction d, String title, String subtitle) => Card(child: Padding(padding: const EdgeInsets.all(20), child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [Text(title, style: Theme.of(context).textTheme.titleLarge), const SizedBox(height: 8), Text(subtitle), const SizedBox(height: 16), Wrap(spacing: 8, runSpacing: 8, children: [FilledButton.tonalIcon(onPressed: () => pick(d, Source.photos), icon: const Icon(Icons.photo_library_outlined), label: Text(t('从照片中选择', 'Choose Photos'))), OutlinedButton.icon(onPressed: () => pick(d, Source.files), icon: const Icon(Icons.folder_open), label: Text(t('从文件中选择', 'Choose Files'))), if (d == Direction.motionToLive) TextButton(onPressed: () => pick(d, Source.scan), child: Text(t('扫描相册', 'Scan Library')))] )])));
  Widget selection(bool wide) {
    final available = photos.where((e) => e.unavailableReason == null).toList();
    final all = available.isNotEmpty && available.every((e) => selected.contains(e.id));
    final controls = Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [if (scanPage) scanCard(), Row(children: [Expanded(child: Text(t('已选 ${selected.length} / ${photos.length} 张', '${selected.length} of ${photos.length} selected'))), TextButton(onPressed: available.isEmpty ? null : () => setState(() { if (all) { selected.clear(); } else { selected.addAll(available.map((e) => e.id)); } }), child: Text(scanning ? t('全选已找到的', 'Select All Found') : all ? t('取消全选', 'Deselect All') : t('全选', 'Select All')))]), if (available.length != photos.length) Text(t('${photos.length - available.length} 张不能转换', "${plural(photos.length - available.length, 'photo')} can't be converted")), if (direction == Direction.liveToMotion) ...[const SizedBox(height: 20), Text(t('对方的手机品牌', 'Their phone brand')), Wrap(spacing: 8, children: ['Google', 'Xiaomi', 'Samsung', 'HUAWEI', 'HONOR', 'OPPO', 'vivo'].map((b) => ChoiceChip(label: Text(b), selected: b == brand, onSelected: (_) => setState(() => brand = b))).toList())], const SizedBox(height: 20), FilledButton(onPressed: selected.isEmpty || scanning ? null : () => start(photos.where((e) => selected.contains(e.id)).toList()), child: Text(selected.isEmpty ? t('转换', 'Convert') : t('转换 ${selected.length} 张', 'Convert ${plural(selected.length, 'Photo')}')))]);
    final grid = available.isEmpty && scanning ? const SizedBox.shrink() : available.isEmpty ? Center(child: Padding(padding: const EdgeInsets.all(24), child: Column(mainAxisSize: MainAxisSize.min, children: [const Icon(Icons.photo_library_outlined, size: 36), Text(scanPage && scanStopped ? t('扫描已停止', 'Scan stopped') : scanPage && !scanFailed ? t('没有找到动态照片', 'No Motion Photos Found') : scanFailed ? t('扫描未完成', 'Scan could not be completed') : t('没有可以转换的照片', 'No Photos to Convert')), Text(t('分享或压缩可能会让照片丢失动态内容。试试原始文件。', 'Sharing or compression may remove motion data. Try the original files.')), TextButton(onPressed: () => pick(direction, Source.files), child: Text(t('从文件中选择', 'Choose Files')))]))) : GridView.builder(shrinkWrap: true, physics: const NeverScrollableScrollPhysics(), itemCount: photos.length, gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(crossAxisCount: MediaQuery.textScalerOf(context).scale(1) >= 1.5 ? 2 : wide ? 4 : 3, crossAxisSpacing: 4, mainAxisSpacing: 4), itemBuilder: (context, i) { final p = photos[i]; final chosen = selected.contains(p.id); return Semantics(button: true, selected: chosen, enabled: p.unavailableReason == null, label: '${p.name}, ${p.unavailableReason ?? (chosen ? t('已选中', 'Selected') : t('未选中', 'Not selected'))}', child: InkWell(onTap: p.unavailableReason != null ? null : () => setState(() { if (!selected.add(p.id)) selected.remove(p.id); }), child: Opacity(opacity: p.unavailableReason == null ? 1 : .4, child: Container(decoration: BoxDecoration(color: Colors.black12, borderRadius: BorderRadius.circular(8), border: chosen ? Border.all(color: Theme.of(context).colorScheme.primary, width: 3) : null), child: Stack(fit: StackFit.expand, children: [if (p.thumbnail != null) ClipRRect(borderRadius: BorderRadius.circular(6), child: Image.memory(p.thumbnail!, fit: BoxFit.cover)) else Center(child: Text(p.name, maxLines: 2, overflow: TextOverflow.ellipsis, textAlign: TextAlign.center)), Positioned(right: 4, top: 4, child: Icon(p.unavailableReason != null ? Icons.warning_amber : chosen ? Icons.check_circle : Icons.circle_outlined, color: chosen ? Theme.of(context).colorScheme.primary : Colors.white)), if (p.unavailableReason != null) Positioned(bottom: 4, left: 4, right: 4, child: Text(p.unavailableReason!, maxLines: 2))]))))); });
    return SingleChildScrollView(padding: const EdgeInsets.all(20), child: wide ? Row(crossAxisAlignment: CrossAxisAlignment.start, children: [Expanded(flex: 4, child: controls), const SizedBox(width: 24), Expanded(flex: 6, child: grid)]) : Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [controls, const SizedBox(height: 16), grid]));
  }
  Widget scanCard() {
    final p = scanProgress;
    final message = scanning
        ? (p == null || p.total <= 0 ? t('正在检查照片', 'Checking photos') : t('已检查 ${p.completed} / ${p.total} 张', 'Checked ${p.completed} of ${p.total} photos'))
        : scanFailed ? t('扫描未完成', 'Scan could not be completed')
        : scanStopped ? t('扫描已停止。', 'Scan stopped.')
        : p == null ? t('扫描完成', 'Scan complete') : t('已检查 ${p.completed} 张，找到 ${p.matches} 张动态照片', 'Checked ${p.completed} photos, found ${p.matches} Motion Photos');
    return Card(child: Padding(padding: const EdgeInsets.all(16), child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      Text(t('只检查你允许访问的照片原件。', 'Only photos you allow access to are checked.'), style: Theme.of(context).textTheme.bodyMedium),
      const SizedBox(height: 8), Text(message, style: Theme.of(context).textTheme.titleMedium),
      if (scanning) ...[const SizedBox(height: 12), Semantics(label: t('正在扫描照片', 'Scanning photos'), value: message, child: LinearProgressIndicator(value: p == null || p.total <= 0 ? null : (p.completed / p.total).clamp(0.0, 1.0))),
        TextButton(onPressed: scanCancelling ? null : () => cancelScan(), child: Text(scanCancelling ? t('正在停止…', 'Stopping…') : t('取消扫描', 'Cancel Scan')))],
      if (p != null && p.issues > 0) Text(t('${p.issues} 张无法检查', '${p.issues} photos could not be checked')),
      if (!scanning) TextButton(onPressed: () => scan(direction), child: Text(t('重新扫描', 'Scan Again'))),
    ])));
  }
  Widget batchList() => Column(children: batch.where((e) => filter == 'all' || (filter == 'success' && e.status == ItemStatus.success) || (filter == 'failed' && e.status == ItemStatus.failed)).map((e) { final (icon, status) = switch (e.status) { ItemStatus.waiting => (Icons.schedule, t('等待', 'Waiting')), ItemStatus.converting => (Icons.sync, t('转换中', 'Converting')), ItemStatus.success => (Icons.check_circle, t('完成', 'Done')), ItemStatus.failed => (Icons.error, t('失败', 'Failed')), ItemStatus.skipped => (Icons.remove_circle_outline, t('未转换', 'Not Converted')) }; return ListTile(leading: e.photo.thumbnail == null ? const Icon(Icons.photo_outlined) : Image.memory(e.photo.thumbnail!, width: 44, height: 44, fit: BoxFit.cover), title: Text(e.photo.name), subtitle: Text(e.reason == null ? status : '$status\n${e.reason}'), trailing: page == PageStep.results && direction == Direction.liveToMotion && e.status == ItemStatus.success ? IconButton(tooltip: t('分享', 'Share'), icon: const Icon(Icons.ios_share), onPressed: busy ? null : () => perform(() => widget.service.share([e]))) : Icon(icon)); }).toList());
  Widget progress(bool wide) {
    final current = batch.where((e) => e.status == ItemStatus.converting).toList();
    final summary = Card(child: Padding(padding: const EdgeInsets.all(20), child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [Text(t('已处理 $processed / ${batch.length} 张', 'Processed $processed of ${batch.length} photos'), style: Theme.of(context).textTheme.titleLarge), const SizedBox(height: 12), Semantics(label: t('转换进度', 'Conversion progress'), value: '$processed / ${batch.length}', child: LinearProgressIndicator(value: batch.isEmpty ? null : processed / batch.length)), if (current.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 12), child: Text(current.first.photo.name, maxLines: 1, overflow: TextOverflow.ellipsis)), const SizedBox(height: 12), OutlinedButton(onPressed: stop, child: Text(t('停止', 'Stop')))])));
    return SingleChildScrollView(padding: const EdgeInsets.all(20), child: wide ? Row(crossAxisAlignment: CrossAxisAlignment.start, children: [Expanded(flex: 6, child: batchList()), const SizedBox(width: 24), Expanded(flex: 4, child: summary)]) : Column(children: [summary, batchList()]));
  }
  Widget results(bool wide) {
    final ok = successes.length, unsaved = unsavedSuccesses, failed = failures.length, skipped = batch.where((e) => e.status == ItemStatus.skipped).length;
    final summary = Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [Icon(ok == 0 ? Icons.error_outline : failed > 0 ? Icons.warning_amber : Icons.check_circle, size: 56, color: ok == 0 ? Colors.red : failed > 0 ? Colors.orange : Colors.green), const SizedBox(height: 12), Text(ok == 0 ? t('没能转换这些照片', "Couldn't Convert These Photos") : failed > 0 ? t('$ok 张已转换，$failed 张失败', '$ok converted, $failed failed') : t('$ok 张照片已转换', '${plural(ok, 'photo')} converted'), textAlign: TextAlign.center, style: Theme.of(context).textTheme.titleLarge), if (skipped > 0) Text(t('$skipped 张未转换', '$skipped not converted'), textAlign: TextAlign.center), if (ok > 0 && failed > 0) Padding(padding: const EdgeInsets.symmetric(vertical: 16), child: SegmentedButton<String>(segments: [ButtonSegment(value: 'all', label: Text(t('全部', 'All'))), ButtonSegment(value: 'success', label: Text(t('成功', 'Converted'))), ButtonSegment(value: 'failed', label: Text(t('失败', 'Failed')))], selected: {filter}, onSelectionChanged: (v) => setState(() => filter = v.first))), const SizedBox(height: 20), if (failed > 0) OutlinedButton(onPressed: busy ? null : () { filter = 'all'; start(failures.map((e) => e.photo).toList(), retained: batch.where((e) => e.status != ItemStatus.failed).toList()); }, child: Text(t('重试失败的 $failed 张', 'Retry ${plural(failed, 'Failed Photo')}'))), if (ok > 0 && direction == Direction.motionToLive) FilledButton(onPressed: busy || unsaved.isEmpty ? null : () => perform(() => widget.service.saveToPhotos(unsaved), savedItems: unsaved), child: Text(unsaved.isEmpty ? t('已存入照片', 'Saved to Photos') : unsaved.length == ok ? t('全部存入照片（$ok）', 'Save All $ok to Photos') : t('存入剩余照片（${unsaved.length}）', 'Save Remaining ${unsaved.length} to Photos'))), if (ok > 0 && direction == Direction.liveToMotion) FilledButton.tonal(onPressed: busy ? null : () => perform(() => widget.service.share(successes), markHandled: true), child: Text(direction == Direction.liveToMotion ? t('分享 $ok 个文件', 'Share ${plural(ok, 'File')}') : t('分享', 'Share'))), if (ok > 0 && direction == Direction.liveToMotion) TextButton(onPressed: busy ? null : () => perform(() => widget.service.saveToFiles(successes), markHandled: true), child: Text(t('存到“文件”', 'Save to Files')))]);
    return SingleChildScrollView(padding: const EdgeInsets.all(20), child: wide ? Row(crossAxisAlignment: CrossAxisAlignment.start, children: [Expanded(flex: 6, child: batchList()), const SizedBox(width: 24), Expanded(flex: 4, child: summary)]) : Column(children: [summary, batchList()]));
  }
}
