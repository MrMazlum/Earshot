// DEMO FRAMES — the README's moving picture, rendered from the real screens.
//
// Run: flutter test test/gen_demo.dart
// Out: /tmp/earshot-demo/frame-####.png, which tools/demo/make_demo.sh turns into a GIF.
//
// Why frames and not a screen recording: a recording needs a phone, a PC, a voice and a cable,
// and it has to be redone every time the screen changes. This drives the same widget tree the
// phone runs, through the same event channel the service pushes to, so the GIF is a picture of
// the code rather than a picture of one afternoon. What it cannot show is sound — for that there
// is no substitute for a real recording, and the README does not pretend otherwise.
//
// What it shows, in order: the sweep's result with two machines already named, the microphone row
// opened on its two sources and its rate, Start pressed, the quiet screen with its ring following a
// voice, detail asked for mid-session, and the cleaning switch turned off and back on while the PC
// says what is actually happening. It ends muted and then live again, so the last frame is a
// working session rather than a stopped one.
//
// The level sequence below is an envelope of ordinary speech — syllables, and the gaps between
// them — not a sine wave, because a ring that pulses evenly looks like a loading spinner and tells
// the reader nothing about what it is for.
//
// 🔴 The cleaning line is never drawn from the switch alone. The switch is a wish that travels to
// the PC; the line under it is the PC's own answer. They are emitted separately here for the same
// reason the app keeps them apart — this app has claimed falsely about cleaning before.

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:earshot/main.dart';

/// The owner's phone: 1220x2712 at density 3.0, and that is what is rendered.
///
/// It used to render at 1.0, on the reasoning that the GIF was 260 px wide and a 3x render was
/// nine times the pixels to throw away. That reasoning produced a 260 px picture, which Reddit's
/// phone app upscales to the width of the screen — so the demo looked soft and the text looked
/// broken on the device most people read it on. Render at the phone's real density and let each
/// output pick its own size.
const _size = Size(406.7, 904);
const _dpr = 3.0;
const _outDir = '/tmp/earshot-demo';

const _rootKey = ValueKey('shot-root');
const _control = MethodChannel('earshot/control');
const _events = EventChannel('earshot/events');

/// 10 frames a second. Fast enough that the ring moves, slow enough that the GIF stays small.
const _frameMs = 100;

late Map<String, Object?> _prefs;
late Map<String, Object?> _network;
int _frame = 0;

/// What the microphone hears over about four seconds of someone talking: a word, a gap, two more
/// words, a breath. Hand-written rather than generated, because the point is that it reads as
/// speech and not as an animation.
const _speech = <double>[
  0.00, 0.08, 0.34, 0.61, 0.72, 0.55, 0.28, 0.12, 0.04, 0.02,
  0.00, 0.00, 0.18, 0.47, 0.66, 0.58, 0.69, 0.74, 0.52, 0.21,
  0.06, 0.02, 0.00, 0.11, 0.39, 0.57, 0.43, 0.24, 0.09, 0.03,
  0.00, 0.00, 0.00, 0.14, 0.44, 0.70, 0.81, 0.63, 0.35, 0.15,
  0.05, 0.01, 0.00, 0.00, 0.07, 0.26, 0.41, 0.30, 0.13, 0.04,
];

Future<void> _loadFonts() async {
  final fonts = _materialFonts();
  final families = {
    'Roboto': ['Roboto-Regular.ttf', 'Roboto-Medium.ttf', 'Roboto-Bold.ttf'],
    'MaterialIcons': ['MaterialIcons-Regular.otf'],
  };
  for (final entry in families.entries) {
    final loader = FontLoader(entry.key);
    for (final name in entry.value) {
      final file = File('${fonts.path}/$name');
      if (!file.existsSync()) {
        throw StateError('gen_demo: ${file.path} is missing.');
      }
      loader.addFont(Future.value(file.readAsBytesSync().buffer.asByteData()));
    }
    await loader.load();
  }

  final monoLoader = FontLoader('JetBrainsMono');
  for (final name in [
    'JetBrainsMonoNL-Regular.ttf',
    'JetBrainsMonoNL-Medium.ttf',
    'JetBrainsMonoNL-Bold.ttf',
  ]) {
    final file = File('assets/fonts/$name');
    if (!file.existsSync()) {
      throw StateError('gen_demo: ${file.path} is missing.');
    }
    monoLoader.addFont(Future.value(file.readAsBytesSync().buffer.asByteData()));
  }
  await monoLoader.load();
}

Directory _materialFonts() {
  var dir = File(Platform.resolvedExecutable).parent;
  for (var up = 0; up < 8; up++) {
    final candidate = Directory('${dir.path}/material_fonts');
    if (candidate.existsSync()) return candidate;
    final nested = Directory('${dir.path}/artifacts/material_fonts');
    if (nested.existsSync()) return nested;
    dir = dir.parent;
  }
  throw StateError('gen_demo: could not find the SDK material_fonts directory.');
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    final out = Directory(_outDir);
    if (out.existsSync()) out.deleteSync(recursive: true);
    out.createSync(recursive: true);
    await _loadFonts();
  });

  setUp(() {
    // Two remembered machines, which is what the list looks like for anybody who has used it
    // twice — and the state that explains the screen fastest to a reader who has never seen it.
    _prefs = {
      'host': '192.168.1.20',
      'port': 47811,
      'source': 7,
      'rate': 48000,
      'known': '[{"host":"192.168.1.20","port":47811,"name":"Windows","seen":2},'
          '{"host":"192.168.1.37","port":47811,"name":"Linux","seen":1}]',
    };
    _network = {'transport': 'wifi', 'ip': '192.168.1.9', 'prefix': 24};

    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
      MethodChannel(_events.name, _events.codec),
      (call) async => null,
    );

    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(_control, (call) async {
      switch (call.method) {
        case 'getPrefs':
          return _prefs;
        case 'getNetwork':
          return _network;
        case 'isRunning':
        case 'isMuted':
          return false;
        default:
          return true;
      }
    });
  });

  testWidgets('the whole thing, as frames', (tester) async {
    await _open(tester);
    var packets = 0;

    // The panel as it opens: the sweep has answered and the two machines it found are named, which
    // is the whole of what anybody has to do before pressing Start.
    await _hold(tester, 12);

    // The microphone row, opened. Both sources, the rate, and the cleaning switch.
    await _tap(tester, find.text('voice call'));
    await _hold(tester, 14);

    // The other source picked and put back. The picker is the one experiment this app exists to
    // run, and a still picture cannot show that it moves.
    await _tap(tester, find.text('plain mic'));
    await _hold(tester, 7);
    await _tap(tester, find.text('voice call'));
    await _hold(tester, 7);

    // Start pressed, and the session confirmed by a PC that is actually holding audio.
    await _tap(tester, find.text('START STREAMING'));
    await _emit(tester, {'event': 'started', 'rate': 48000, 'source': 7});
    await _stats(tester, level: 0.0, packets: packets);

    // The route transition between the two screens. Captured rather than skipped: it is the
    // moment the app earns the quiet screen, and it is half a second of the real thing.
    await _hold(tester, 6);

    // Four seconds of talking.
    for (var i = 0; i < _speech.length; i++) {
      packets += 50;
      await _stats(tester, level: _speech[i], packets: packets);
      await _shoot(tester);
    }

    // Detail asked for mid-session. The microphone row is still open from before, and while a
    // session runs the cleaning switch is the only thing in it that can still be touched — so it
    // is what the row opens on.
    await _tap(tester, find.text('tap anywhere for details'));
    packets = await _holdLive(tester, 14, packets: packets);

    // Cleaning turned off: the switch empties, and the PC's own line follows it to 'off'. The two
    // are separate messages because in the real app they can disagree.
    await _tap(tester, find.text('[x]  clean the noise'));
    packets = await _holdLive(tester, 10, packets: packets, cleaning: false);

    // And back on.
    await _tap(tester, find.text('[ ]  clean the noise'));
    packets = await _holdLive(tester, 10, packets: packets);

    // Back to the quiet screen, muted, and then live again — so the picture ends on a session that
    // is working rather than on one that is stopped.
    await _tap(tester, find.text('hide'));
    packets = await _holdLive(tester, 4, packets: packets);

    await _tap(tester, find.byIcon(Icons.mic_none));
    await _emit(tester, {'event': 'muted', 'muted': true});
    await _hold(tester, 9);

    await _tap(tester, find.byIcon(Icons.mic_off));
    await _emit(tester, {'event': 'muted', 'muted': false});
    for (var i = 0; i < 16; i++) {
      packets += 50;
      await _stats(tester, level: _speech[i], packets: packets);
      await _shoot(tester);
    }
  });
}

/// Taps something and captures the frame it lands on, so a press is never invisible.
Future<void> _tap(WidgetTester tester, Finder finder) async {
  await tester.tap(finder);
  await tester.pump(const Duration(milliseconds: _frameMs));
  await _shoot(tester);
}

/// Holds a screen while a session is running — which means keeping the PC's replies coming, or
/// the link would go quiet on its own and the panel would start saying so.
///
/// Returns the packet count it reached, so the next beat carries on from it rather than counting
/// backwards, which a reader does notice.
Future<int> _holdLive(
  WidgetTester tester,
  int frames, {
  required int packets,
  bool cleaning = true,
}) async {
  var count = packets;
  for (var i = 0; i < frames; i++) {
    count += 50;
    await _stats(tester, level: _speech[i % _speech.length] * 0.5,
        packets: count, cleaning: cleaning);
    await _shoot(tester);
  }
  return count;
}

Future<void> _open(WidgetTester tester) async {
  tester.view
    ..physicalSize = Size(_size.width * _dpr, _size.height * _dpr)
    ..devicePixelRatio = _dpr;
  addTearDown(tester.view.reset);

  await tester.pumpWidget(
    const RepaintBoundary(key: _rootKey, child: EarshotApp()),
  );
  await tester.pumpAndSettle();
}

/// Holds the current screen for [frames] frames, advancing the clock so anything that moves on
/// its own — the ring's slow breath — keeps moving while nothing else changes.
Future<void> _hold(WidgetTester tester, int frames) async {
  for (var i = 0; i < frames; i++) {
    await tester.pump(const Duration(milliseconds: _frameMs));
    await _shoot(tester);
  }
}

Future<void> _stats(
  WidgetTester tester, {
  required double level,
  required int packets,
  bool cleaning = true,
}) =>
    _emit(tester, {
      'event': 'stats',
      'packets': packets,
      // 90 bytes a packet — an Opus voice frame plus its header, which the panel reads back as
      // about 36 kbps. 🔴 It used to be 1936, the size of a RAW PCM frame, so the signal row
      // advertised ~770 kbps: the debug fallback, not the format. The protocol note is explicit
      // that Opus is 32–40 kbps against roughly 770 for the same voice uncompressed, and a demo
      // that shows the wrong one of those two numbers argues against the thing it is demonstrating.
      'bytes': packets * 90,
      'level': level,
      'rate': 48000,
      'source': 7,
      'answeredMsAgo': 20,
      'pcBufferedMs': 61.0,
      'pcCanClean': true,
      'pcCleaning': cleaning,
    });

/// Pushes an event up the same channel the service uses, then advances one frame.
///
/// ⚠️ Not pumpAndSettle: the ring breathes for as long as the session lasts, so settling waits
/// for an animation that never ends.
Future<void> _emit(WidgetTester tester, Map<String, Object?> event) async {
  await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
      .handlePlatformMessage(
    _events.name,
    const StandardMethodCodec().encodeSuccessEnvelope(event),
    (_) {},
  );
  await tester.pump(const Duration(milliseconds: _frameMs));
}

Future<void> _shoot(WidgetTester tester) async {
  final boundary = tester.renderObject<RenderRepaintBoundary>(
    find.byKey(_rootKey),
  );
  await tester.runAsync(() async {
    final image = await boundary.toImage(pixelRatio: _dpr);
    final data = await image.toByteData(format: ui.ImageByteFormat.png);
    final name = _frame.toString().padLeft(4, '0');
    File('$_outDir/frame-$name.png').writeAsBytesSync(data!.buffer.asUint8List());
    image.dispose();
  });
  _frame++;
}
