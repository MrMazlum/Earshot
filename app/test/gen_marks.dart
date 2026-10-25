// Candidate Earshot marks — an ear with a bullet through it — drawn for a decision.
//
// Four takes on the same pun, deliberately different from one another rather than four versions of
// one idea. Each is drawn large and again at launcher size, because a mark that only works at
// 512 px is not a mark: the only place anybody sees it is a 48 px icon.
//
//     flutter test test/gen_marks.dart        → /tmp/earshot-marks/
//
// Drawn in Dart rather than in the Python icon tool for one practical reason: the machine this is
// developed on has no Pillow, and this harness already renders PNGs. The winner gets ported into
// tools/icon/make_icons.py, which is what produces the real launcher assets.

import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

const _outDir = '/tmp/earshot-marks';
const _size = 512.0;

/// The app's current seed colour, kept so the marks are judged on shape, not on a new palette.
/// Colour comes after the mark is chosen.
const _green = Color(0xFF3DDC97);
const _backdrop = Color(0xFF0B1310);

/// The ear, drawn as a spiral.
///
/// An anatomical ear does not survive being 48 px tall, and a geometric one reads as a bracket. A
/// spiral reads as both at once: it is the shape of a cochlea and the shape of a sound wave, and
/// it is the one glyph that says "hearing" with no text under it.
///
/// Angles are Flutter's — 0 is 3 o'clock, a positive sweep goes clockwise — and the spiral is
/// walked by hand because `Path` has no spiral of its own.
Path _spiral({
  required Offset centre,
  required double innerRadius,
  required double outerRadius,
  required double turns,
  double startAngle = -math.pi / 2,
}) {
  final path = Path();
  const steps = 240;
  final total = turns * 2 * math.pi;
  for (var i = 0; i <= steps; i++) {
    final t = i / steps;
    final angle = startAngle + total * t;
    final radius = innerRadius + (outerRadius - innerRadius) * t;
    final point = centre + Offset(math.cos(angle) * radius, math.sin(angle) * radius);
    if (i == 0) {
      path.moveTo(point.dx, point.dy);
    } else {
      path.lineTo(point.dx, point.dy);
    }
  }
  return path;
}

/// The ear: a spiral, with a lobe closing its outer end.
void _ear(
  Canvas canvas,
  double s, {
  double rim = 40,
  double turns = 1.35,
  bool lobe = true,
  Color colour = _green,
}) {
  final centre = Offset(256 * s, 250 * s);
  final stroke = Paint()
    ..color = colour
    ..style = PaintingStyle.stroke
    ..strokeCap = StrokeCap.round
    ..strokeWidth = rim * s;
  canvas.drawPath(
    _spiral(
      centre: centre,
      innerRadius: 34 * s,
      outerRadius: 168 * s,
      turns: turns,
    ),
    stroke,
  );
  // The canal: the one solid in the mark, so the middle does not read as empty.
  canvas.drawCircle(centre, rim * 0.42 * s, Paint()..color = colour);
  if (lobe) {
    // Where the spiral ends, thickened — an ear ends in a lobe, and the mark needs one heavy
    // terminal or it spins forever.
    final end = _spiralPoint(centre, 34 * s, 168 * s, turns, 1.0);
    canvas.drawCircle(end, rim * 0.78 * s, Paint()..color = colour);
  }
}

Offset _spiralPoint(Offset centre, double inner, double outer, double turns, double t) {
  final angle = -math.pi / 2 + turns * 2 * math.pi * t;
  final radius = inner + (outer - inner) * t;
  return centre + Offset(math.cos(angle) * radius, math.sin(angle) * radius);
}

/// The ear, built from nested arcs — the other way of drawing one.
///
/// Where [_ear] is a spiral (cochlea, sound wave), this is the outline: a rim, the ridge inside it
/// and a lobe. It is more obviously an ear and less obviously a sound, which is the trade the two
/// styles exist to put side by side.
void _earArcs(
  Canvas canvas,
  double s, {
  double rim = 42,
  bool ridge = true,
  bool lobe = true,
  Color colour = _green,
}) {
  final stroke = Paint()
    ..color = colour
    ..style = PaintingStyle.stroke
    ..strokeCap = StrokeCap.round
    ..strokeWidth = rim * s;
  // The helix: a C opening to the right, from the lower right round the back to the upper right.
  canvas.drawArc(Rect.fromLTRB(128 * s, 86 * s, 384 * s, 396 * s), math.pi / 2.6,
      math.pi * 1.28, false, stroke);
  if (ridge) {
    stroke.strokeWidth = rim * 0.6 * s;
    canvas.drawArc(Rect.fromLTRB(196 * s, 158 * s, 330 * s, 326 * s), math.pi / 2.2,
        math.pi * 1.15, false, stroke);
  }
  if (lobe) {
    // The lobe, hanging off the lower end of the rim and heavier than it, which is what stops the
    // whole thing reading as a bracket.
    canvas.drawCircle(Offset(214 * s, 384 * s), rim * 0.92 * s, Paint()..color = colour);
  }
  canvas.drawCircle(Offset(268 * s, 250 * s), rim * 0.5 * s, Paint()..color = colour);
}

/// The channel a shot leaves behind it: whatever it passed through is simply gone.
///
/// Erasing before drawing is what makes the bullet read as *through* rather than *in front of*.
void _channel(Canvas canvas, Offset from, Offset to, double width) {
  canvas.drawLine(
    from,
    to,
    Paint()
      ..color = _backdrop
      ..strokeCap = StrokeCap.butt
      ..strokeWidth = width,
  );
}

/// The spiral ear with a bullet through it, nose already out the far side.
///
/// The literal reading of the name: an ear, a bullet, and the gap it left on its way through.
class AnatomicalMark extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width / _size;
    _earArcs(canvas, s);
    // Low on the left to high on the right, so the nose clears the rim with the tail still
    // outside: the moment of passing through, rather than before or after it.
    final from = Offset(28 * s, 356 * s);
    final to = Offset(492 * s, 168 * s);
    _channel(canvas, from, to, 78 * s);
    _bullet(canvas, from: from, to: to, length: 230 * s, calibre: 56 * s);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

/// The same spiral, crossed by a tracer rather than a bullet.
///
/// The shot is motion, not object: a wedge thin at the tail and blunt at the head. Nothing in it
/// is a weapon, which matters if this ever goes near an app store.
class StreakMark extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width / _size;
    _ear(canvas, s, rim: 36, turns: 1.5);
    final from = Offset(30 * s, 392 * s);
    final to = Offset(492 * s, 132 * s);
    _channel(canvas, from, to, 64 * s);
    final direction = (to - from) / (to - from).distance;
    final normal = Offset(-direction.dy, direction.dx);
    final path = Path()
      ..moveTo((from + normal * 2 * s).dx, (from + normal * 2 * s).dy)
      ..lineTo((from - normal * 2 * s).dx, (from - normal * 2 * s).dy)
      ..lineTo((to - normal * 19 * s).dx, (to - normal * 19 * s).dy)
      ..lineTo((to + normal * 19 * s).dx, (to + normal * 19 * s).dy)
      ..close();
    canvas.drawPath(path, Paint()..color = _green);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

/// The bullet is gone; only the hole and the gap are left.
///
/// The most restrained of the four. Nothing in it is a weapon at all, and the pun lands a second
/// later — which is usually the better joke.
class HoleMark extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width / _size;
    _earArcs(canvas, s, rim: 52);
    // No bullet anywhere: only the gap it left, straight through the rim and out the other side.
    final from = Offset(60 * s, 330 * s);
    final to = Offset(470 * s, 180 * s);
    _channel(canvas, from, to, 46 * s);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

/// One turn, one bullet, nothing else.
///
/// Nearly a letter, and the only one of the four likely to survive being 24 px tall in a
/// notification.
class MonogramMark extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width / _size;
    _earArcs(canvas, s, rim: 60, ridge: false, lobe: false);
    final from = Offset(120 * s, 252 * s);
    final to = Offset(452 * s, 252 * s);
    _channel(canvas, from, to, 96 * s);
    _bullet(canvas, from: from, to: to, length: 210 * s, calibre: 66 * s);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

/// A bullet silhouette: a flat base and an ogive nose, pointing from [from] towards [to].
///
/// The nose is two quadratic curves rather than half an ellipse — an ellipse gives a dome that
/// reads as a pill, and a pill is not a bullet.
void _bullet(
  Canvas canvas, {
  required Offset from,
  required Offset to,
  required double length,
  required double calibre,
}) {
  final direction = (to - from) / (to - from).distance;
  canvas.save();
  canvas.translate(to.dx, to.dy);
  canvas.rotate(math.atan2(direction.dy, direction.dx));
  final half = calibre / 2;
  final nose = calibre * 1.25;
  final path = Path()
    ..moveTo(-length, -half)
    ..lineTo(-nose, -half)
    ..quadraticBezierTo(-nose * 0.2, -half * 0.92, 0, 0)
    ..quadraticBezierTo(-nose * 0.2, half * 0.92, -nose, half)
    ..lineTo(-length, half)
    ..close();
  canvas.drawPath(path, Paint()..color = _green);
  canvas.restore();
}

final _marks = <String, CustomPainter>{
  'a-anatomical': AnatomicalMark(),
  'b-streak': StreakMark(),
  'c-hole': HoleMark(),
  'd-monogram': MonogramMark(),
};

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() => Directory(_outDir).createSync(recursive: true));

  testWidgets('every candidate mark, large and at launcher size', (tester) async {
    final shots = <String, ui.Image>{};
    for (final entry in _marks.entries) {
      shots[entry.key] = await _render(tester, entry.value);
    }

    for (final entry in shots.entries) {
      await _write(entry.value, '$_outDir/earshot-mark-${entry.key}.png');
    }
    await _sheet(shots);

    // Two identical frames would mean the painters are not actually being used — the cheapest
    // possible check, and one this project has been bitten by before.
    final bytes = <String, int>{};
    for (final entry in shots.entries) {
      final data = await entry.value.toByteData(format: ui.ImageByteFormat.png);
      bytes[entry.key] = data!.lengthInBytes;
    }
    expect(bytes.values.toSet().length, bytes.length,
        reason: 'two marks rendered to the same bytes, so one of them did not draw');
  });
}

Future<ui.Image> _render(WidgetTester tester, CustomPainter painter) async {
  late ui.Image image;
  await tester.runAsync(() async {
    final recorder = ui.PictureRecorder();
    final canvas = Canvas(recorder);
    canvas.drawRect(const Rect.fromLTWH(0, 0, _size, _size), Paint()..color = _backdrop);
    painter.paint(canvas, const Size(_size, _size));
    image = await recorder.endRecording().toImage(_size.toInt(), _size.toInt());
  });
  return image;
}

Future<void> _write(ui.Image image, String path) async {
  final data = await image.toByteData(format: ui.ImageByteFormat.png);
  File(path).writeAsBytesSync(data!.buffer.asUint8List());
  // ignore: avoid_print
  print('shot $path');
}

/// All four side by side, each large and at launcher size — the comparison that decides it.
Future<void> _sheet(Map<String, ui.Image> shots) async {
  const pad = 24.0;
  const small = 96.0;
  final width = pad + (_size + pad) * shots.length;
  final height = _size + small + pad * 3;

  final recorder = ui.PictureRecorder();
  final canvas = Canvas(recorder);
  canvas.drawRect(Rect.fromLTWH(0, 0, width, height), Paint()..color = _backdrop);
  var i = 0;
  for (final image in shots.values) {
    final x = pad + i * (_size + pad);
    canvas.drawImageRect(
      image,
      const Rect.fromLTWH(0, 0, _size, _size),
      Rect.fromLTWH(x, pad, _size, _size),
      Paint(),
    );
    canvas.drawImageRect(
      image,
      const Rect.fromLTWH(0, 0, _size, _size),
      Rect.fromLTWH(x + _size / 2 - small / 2, _size + pad * 2, small, small),
      Paint()..filterQuality = FilterQuality.medium,
    );
    i++;
  }
  final picture = recorder.endRecording();
  final sheet = await picture.toImage(width.toInt(), height.toInt());
  await _write(sheet, '$_outDir/earshot-marks-sheet.png');
}
