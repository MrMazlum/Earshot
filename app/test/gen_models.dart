// The three finished looks, photographed side by side so one can be chosen.
//
//     flutter test test/gen_models.dart      → /tmp/earshot-models/
//
// His instruction, 2026-10-25: *"generate 3 final models at the end. Try to use different coloring
// and ui ux aspects."* So these are not three shades of the same screen — they are three answers to
// the same question, and theme.dart says what each one is betting on.
//
// Every look is rendered in the four states that matter: the screen you set up on, the confirmed
// session, that session a few seconds later once it has gone quiet, and the one that is broken.
// A look that is only photographed working has not been looked at.

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:earshot/collapsed_view.dart';
import 'package:earshot/main.dart';
import 'package:earshot/theme.dart';

const _outDir = '/tmp/earshot-models';
const _size = Size(390, 844);
const _dpr = 3.0;
const _rootKey = ValueKey('model-root');

const _control = MethodChannel('earshot/control');
const _events = EventChannel('earshot/events');

Map<String, dynamic> _prefs = {};
Map<String, dynamic> _network = {};

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    Directory(_outDir).createSync(recursive: true);
    await _loadFonts();
  });

  setUp(() {
    _prefs = {'host': '192.168.1.13', 'port': 47811, 'source': 7, 'rate': 48000};
    _network = {
      'transport': 'wifi',
      'ip': '192.168.1.9',
      'prefix': 24,
      'localIp': '192.168.1.9',
      'localPrefix': 24,
    };
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

  for (final palette in palettes) {
    final slug = palette.name.toLowerCase();

    testWidgets('${palette.name}: the screen you set up on', (tester) async {
      await _openApp(tester, palette);
      await _shoot(tester, '$slug-1-setting-up');
    });

    testWidgets('${palette.name}: confirmed, just connected', (tester) async {
      await _openWidget(
        tester,
        palette,
        CollapsedView(
          palette: palette,
          state: 'connected',
          level: 0.62,
          elapsed: const Duration(minutes: 7, seconds: 12),
          onExpand: () {},
          onToggleMute: () {},
          settledOverride: false,
        ),
      );
      await _shoot(tester, '$slug-2-connected');
    });

    testWidgets('${palette.name}: the same session, left alone', (tester) async {
      await _openWidget(
        tester,
        palette,
        CollapsedView(
          palette: palette,
          state: 'connected',
          level: 0.22,
          elapsed: const Duration(minutes: 41, seconds: 3),
          onExpand: () {},
          onToggleMute: () {},
          settledOverride: true,
        ),
      );
      await _shoot(tester, '$slug-3-settled');
    });

    testWidgets('${palette.name}: muted', (tester) async {
      await _openWidget(
        tester,
        palette,
        CollapsedView(
          palette: palette,
          state: 'muted',
          level: 0,
          elapsed: const Duration(minutes: 12, seconds: 48),
          onExpand: () {},
          onToggleMute: () {},
          settledOverride: false,
        ),
      );
      await _shoot(tester, '$slug-4-muted');
    });

    testWidgets('${palette.name}: nothing is arriving', (tester) async {
      await _openApp(tester, palette);
      await _emit(tester, {'event': 'started', 'rate': 48000, 'source': 7});
      await _emit(tester, {
        'event': 'stats',
        'packets': 980,
        'bytes': 61000,
        'level': 0.4,
        'rate': 48000,
        'source': 7,
        'answeredMsAgo': -1,
        'pcBufferedMs': 0.0,
      });
      await _shoot(tester, '$slug-5-no-answer');
    });
  }

  testWidgets('the three, side by side', (tester) async {
    // runAsync, because decoding a PNG is real asynchronous work: inside the test's fake clock it
    // never completes and the whole run hangs until the timeout kills it.
    await tester.runAsync(_sheet);
  });
}

Future<void> _openApp(WidgetTester tester, Palette palette) async {
  tester.view
    ..physicalSize = Size(_size.width * _dpr, _size.height * _dpr)
    ..devicePixelRatio = _dpr;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    RepaintBoundary(key: _rootKey, child: EarshotApp(palette: palette)),
  );
  await tester.pumpAndSettle();
}

Future<void> _openWidget(WidgetTester tester, Palette palette, Widget child) async {
  tester.view
    ..physicalSize = Size(_size.width * _dpr, _size.height * _dpr)
    ..devicePixelRatio = _dpr;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    RepaintBoundary(
      key: _rootKey,
      child: MaterialApp(
        debugShowCheckedModeBanner: false,
        theme: palette.theme,
        home: Scaffold(body: child),
      ),
    ),
  );
  // Not pumpAndSettle: the ring breathes forever, and waiting for it to stop is waiting for the
  // thing the picture is of.
  await tester.pump(const Duration(milliseconds: 500));
}

Future<void> _emit(WidgetTester tester, Map<String, dynamic> event) async {
  await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
      .handlePlatformMessage(
    _events.name,
    _events.codec.encodeSuccessEnvelope(event),
    (_) {},
  );
  await tester.pump(const Duration(milliseconds: 60));
}

Future<void> _shoot(WidgetTester tester, String name) async {
  final boundary = tester.renderObject<RenderRepaintBoundary>(find.byKey(_rootKey));
  await tester.runAsync(() async {
    final image = await boundary.toImage(pixelRatio: _dpr);
    final data = await image.toByteData(format: ui.ImageByteFormat.png);
    File('$_outDir/$name.png').writeAsBytesSync(data!.buffer.asUint8List());
    image.dispose();
  });
  // ignore: avoid_print
  print('shot $_outDir/$name.png');
}

/// A contact sheet per look, and the one that decides it: all three, same state, same size.
Future<void> _sheet() async {
  for (final state in ['1-setting-up', '2-connected', '3-settled', '4-muted', '5-no-answer']) {
    final images = <ui.Image>[];
    for (final palette in palettes) {
      final file = File('$_outDir/${palette.name.toLowerCase()}-$state.png');
      if (!file.existsSync()) continue;
      final codec = await ui.instantiateImageCodec(file.readAsBytesSync());
      images.add((await codec.getNextFrame()).image);
    }
    if (images.isEmpty) continue;

    const pad = 40.0;
    final w = images.first.width.toDouble();
    final h = images.first.height.toDouble();
    final width = pad + (w + pad) * images.length;
    final height = h + pad * 2;

    final recorder = ui.PictureRecorder();
    final canvas = Canvas(recorder);
    canvas.drawRect(Rect.fromLTWH(0, 0, width, height), Paint()..color = const Color(0xFF17191B));
    for (var i = 0; i < images.length; i++) {
      canvas.drawImage(images[i], Offset(pad + i * (w + pad), pad), Paint());
    }
    final sheet = await recorder.endRecording().toImage(width.toInt(), height.toInt());
    final data = await sheet.toByteData(format: ui.ImageByteFormat.png);
    File('$_outDir/sheet-$state.png').writeAsBytesSync(data!.buffer.asUint8List());
    // ignore: avoid_print
    print('shot $_outDir/sheet-$state.png');
  }
}

Future<void> _loadFonts() async {
  final fonts = _materialFonts();
  final families = {
    'Roboto': ['Roboto-Regular.ttf', 'Roboto-Medium.ttf', 'Roboto-Bold.ttf'],
    // Without this the icons render as empty boxes, which is a picture of nothing.
    'MaterialIcons': ['MaterialIcons-Regular.otf'],
  };
  for (final entry in families.entries) {
    final loader = FontLoader(entry.key);
    for (final name in entry.value) {
      final file = File('${fonts.path}/$name');
      if (!file.existsSync()) {
        throw StateError('gen_models: ${file.path} is missing.');
      }
      loader.addFont(Future.value(file.readAsBytesSync().buffer.asByteData()));
    }
    await loader.load();
  }
}

/// The SDK's own fonts, found from the running test binary rather than guessed at — a wrong guess
/// here photographs pages of black boxes and calls them screenshots.
Directory _materialFonts() {
  var dir = File(Platform.resolvedExecutable).parent;
  for (var up = 0; up < 8; up++) {
    final candidate = Directory('${dir.path}/material_fonts');
    if (candidate.existsSync()) return candidate;
    final nested = Directory('${dir.path}/artifacts/material_fonts');
    if (nested.existsSync()) return nested;
    dir = dir.parent;
  }
  throw StateError('gen_models: no material_fonts from ${Platform.resolvedExecutable}');
}
