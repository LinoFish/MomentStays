import 'package:flutter/cupertino.dart';

/// Presentation-only Cupertino fallback. Business state and service calls stay
/// outside this boundary so a verified platform implementation can replace it.
class PlatformAction extends StatelessWidget {
  const PlatformAction({super.key, required this.label, required this.onPressed, this.busy = false});
  final String label;
  final VoidCallback? onPressed;
  final bool busy;
  @override
  Widget build(BuildContext context) => CupertinoButton(
    color: const Color(0xfff0a43c), disabledColor: CupertinoColors.systemGrey4,
    borderRadius: BorderRadius.circular(26), onPressed: onPressed,
    child: busy ? Row(mainAxisSize: MainAxisSize.min, mainAxisAlignment: MainAxisAlignment.center, children: [const CupertinoActivityIndicator(), const SizedBox(width: 8), Flexible(child: Text(label))]) : Text(label, style: const TextStyle(color: Color(0xff1f2933), fontWeight: FontWeight.w600)));
}

class PlatformProgress extends StatelessWidget {
  const PlatformProgress({super.key, required this.value, required this.label});
  final double? value;
  final String label;
  @override
  Widget build(BuildContext context) => Semantics(label: label, value: value == null ? null : '${(value! * 100).round()}%', child: value == null ? const Align(alignment: Alignment.centerLeft, child: CupertinoActivityIndicator()) : LayoutBuilder(builder: (context, constraints) => ClipRRect(borderRadius: BorderRadius.circular(2), child: Container(height: 4, color: CupertinoColors.systemGrey4, alignment: Alignment.centerLeft, child: FractionallySizedBox(widthFactor: value!.clamp(0.0, 1.0), child: const ColoredBox(color: Color(0xffa65300)))))));
}

Future<bool> platformConfirm(BuildContext context, {required String title, required String message, required String cancel, required String action}) async => await showCupertinoDialog<bool>(context: context, builder: (c) => CupertinoAlertDialog(title: Text(title), content: Text(message), actions: [CupertinoDialogAction(onPressed: () => Navigator.pop(c, false), child: Text(cancel)), CupertinoDialogAction(isDestructiveAction: true, onPressed: () => Navigator.pop(c, true), child: Text(action))])) ?? false;
