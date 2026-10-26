// Candidate Earshot marks — an ear with a bullet through it — drawn for a decision.
//
//     flutter test test/gen_marks.dart        → /tmp/earshot-marks/
//
// Four takes on one pun, deliberately different from each other. Each is drawn large and again at
// launcher size, because the only place anybody sees a mark is a 48 px icon, and three earlier
// rounds of this were binned for reading at 512 and dissolving at 48.
//
// What those rounds taught, written down so it is not relearned:
//
//   - A **stroked** ear reads as a bracket or an "e". A filled silhouette with the inside cut out
//     reads as an ear, because that is how every ear glyph anybody recognises is drawn.
//   - A bullet drawn *on top of* the ear looks like a badge stuck to it. It has to take a bite out
//     of the shape — the missing channel is what makes it a piercing rather than a sticker.
//   - A horizontal bullet reads as a pencil. Tilt it.
//
// Drawn in Dart rather than in the Python icon tool because this machine has no Pillow and this
// harness already renders PNGs. The winner gets ported to tools/icon/make_icons.py, which is what
// produces the real launcher assets.

import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

const _outDir = '/tmp/earshot-marks';
const _size = 512.0;

const _green = Color(0xFF3DDC97);
const _backdrop = Color(0xFF0B1310);

/// The ear, as a filled silhouette: an outer shape with the inside cut away.
///
/// What makes it an ear rather than a bean: a helix curling over at the top, a notch bitten out of
/// the lower right — the intertragic notch, the most recognisable part of an ear's outline — and a
/// lobe hanging below that.
Path _earOutline() {
  return Path()
    ..moveTo(300, 76)
    ..cubicTo(196, 66, 128, 146, 134, 240)
    ..cubicTo(138, 302, 170, 334, 182, 378)
    ..cubicTo(194, 420, 218, 446, 254, 446)
    ..cubicTo(292, 446, 310, 414, 308, 384)
    ..cubicTo(306, 362, 292, 352, 290, 336)
    ..cubicTo(288, 318, 302, 312, 318, 306)
    ..cubicTo(300, 300, 288, 288, 290, 268)
    ..cubicTo(292, 242, 322, 232, 346, 216)
    ..cubicTo(388, 188, 402, 142, 378, 110)
    ..cubicTo(358, 84, 332, 79, 300, 76)
    ..close();
}

/// The ridge curling around the canal.
///
/// ⚠️ A circle with a bar under it reads as a **keyhole**, which is exactly what the first attempt
/// drew. An ear's inside is a spiral, so the cut is a curl.
Path _earCurl() {
  return Path()
    ..moveTo(296, 168)
    ..cubicTo(246, 150, 192, 184, 192, 236)
    ..cubicTo(192, 286, 228, 306, 240, 338)
    ..cubicTo(250, 364, 244, 392, 226, 398);
}

void _drawEar(Canvas canvas, double s, {double scale = 1.0}) {
  canvas.save();
  canvas.scale(s * scale);
  if (scale != 1.0) {
    // Keep a shrunken ear centred in the same box rather than pinned to the corner.
    canvas.translate(_size * (1 - scale) / (2 * scale), _size * (1 - scale) / (2 * scale));
  }
  canvas.drawPath(_earOutline(), Paint()..color = _green);
  // Cut the inside out: the canal, then the ridge that curls around it. Stroked straight onto the
  // canvas rather than combined into one path — Flutter has no stroke-to-path, and two draws in
  // the backdrop colour do the same job.
  canvas.drawCircle(const Offset(238, 232), 44, Paint()..color = _backdrop);
  canvas.drawPath(
    _earCurl(),
    Paint()
      ..color = _backdrop
      ..style = PaintingStyle.stroke
      ..strokeWidth = 30
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round,
  );
  canvas.restore();
}

/// The channel a shot leaves: whatever it passed through is gone.
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

/// A bullet: flat base, ogive nose, pointing from [from] towards [to].
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

/// **A — the hole.** An ear, and one clean hole punched through the top of it.
///
/// No weapon anywhere in the mark. The pun lands a second after you see it, which is usually the
/// better joke, and it is the only one of the four that could go near an app store unargued.
class HoleMark extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width / _size;
    _drawEar(canvas, s);
    // High on the helix, where the rim is widest — a hole somewhere thin just looks chipped.
    canvas.drawCircle(Offset(330 * s, 136 * s), 30 * s, Paint()..color = _backdrop);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

/// **B — through.** The bullet caught mid-flight: nose out the far side, tail still outside.
///
/// The most literal reading — an ear, a bullet, and the gap it took out on the way.
class ThroughMark extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width / _size;
    _drawEar(canvas, s);
    // A narrow channel: wide enough to read as a hole through the shape, narrow enough that the
    // ear stays one piece. A wide one cut it into three.
    // ⚠️ A shallow bullet lying along the top of the ear reads as the peak of a baseball cap.
    // Steeper, shorter, and crossing the body rather than skimming it.
    final from = Offset(60 * s, 452 * s);
    final to = Offset(436 * s, 92 * s);
    _channel(canvas, from, to, 30 * s);
    _bullet(canvas, from: from, to: to, length: 118 * s, calibre: 32 * s);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

/// **C — earshot.** The ear, the hole, and the sound still arriving from where the shot came.
///
/// *Within earshot* is a distance, and the two faint arcs are the only thing in any of the four
/// that says so.
class DistanceMark extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width / _size;
    final stroke = Paint()
      ..color = _green.withValues(alpha: 0.36)
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round;
    // Centred on the canal and opening away from the ear, so they read as sound arriving rather
    // than as a bracket stuck to the side.
    for (final r in const [
      [168.0, 14.0],
      [214.0, 11.0],
    ]) {
      stroke.strokeWidth = r[1] * s;
      canvas.drawArc(
        Rect.fromCircle(center: Offset(238 * s, 232 * s), radius: r[0] * s),
        math.pi * 0.62,
        math.pi * 0.76,
        false,
        stroke,
      );
    }
    _drawEar(canvas, s);
    canvas.drawCircle(Offset(330 * s, 136 * s), 28 * s, Paint()..color = _backdrop);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

/// **D — the badge.** The ear inside a disc, the shot taking a bite out of both.
///
/// The heaviest of the four and the one that survives being tiny: at 24 px it is still a circle
/// with a notch out of it, which is a shape — where the others are a smudge.
class BadgeMark extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width / _size;
    canvas.drawCircle(
      Offset(256 * s, 256 * s),
      216 * s,
      Paint()
        ..color = _green
        ..style = PaintingStyle.stroke
        ..strokeWidth = 28 * s,
    );
    _drawEar(canvas, s, scale: 0.58);
    final from = Offset(96 * s, 404 * s);
    final to = Offset(430 * s, 108 * s);
    _channel(canvas, from, to, 28 * s);
    _bullet(canvas, from: from, to: to, length: 104 * s, calibre: 28 * s);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

final _marks = <String, CustomPainter>{
  'a-hole': HoleMark(),
  'b-through': ThroughMark(),
  'c-distance': DistanceMark(),
  'd-badge': BadgeMark(),
};

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() => Directory(_outDir).createSync(recursive: true));

  testWidgets('every candidate mark, large and at launcher size', (tester) async {
    final shots = <String, ui.Image>{};
    for (final entry in _marks.entries) {
      shots[entry.key] = await _render(tester, entry.value);
    }
    // runAsync for anything that rasterises or encodes: inside the test's fake clock those futures
    // never complete, and the run hangs until a timeout kills it having written half the pictures.
    final bytes = <String, int>{};
    await tester.runAsync(() async {
      for (final entry in shots.entries) {
        await _write(entry.value, '$_outDir/earshot-mark-${entry.key}.png');
      }
      await _sheet(shots);

      // Two identical frames would mean a painter never ran — the cheapest possible check, and
      // one this project has been bitten by before.
      for (final entry in shots.entries) {
        final data = await entry.value.toByteData(format: ui.ImageByteFormat.png);
        bytes[entry.key] = data!.lengthInBytes;
      }
    });
    expect(bytes.values.toSet().length, bytes.length,
        reason: 'two marks rendered identically, so one of them did not draw');
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
    canvas.drawImageRect(image, const Rect.fromLTWH(0, 0, _size, _size),
        Rect.fromLTWH(x, pad, _size, _size), Paint());
    canvas.drawImageRect(
      image,
      const Rect.fromLTWH(0, 0, _size, _size),
      Rect.fromLTWH(x + _size / 2 - small / 2, _size + pad * 2, small, small),
      Paint()..filterQuality = FilterQuality.medium,
    );
    i++;
  }
  final sheet = await recorder.endRecording().toImage(width.toInt(), height.toInt());
  await _write(sheet, '$_outDir/earshot-marks-sheet.png');
}
