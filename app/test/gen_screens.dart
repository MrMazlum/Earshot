// SCREEN CAPTURE HARNESS — renders the real screen, in the states that matter, at the size of a
// real phone, and writes PNGs so the work can be LOOKED AT before it is called done.
//
// Run: flutter test test/gen_screens.dart
// Out: /tmp/earshot-screens/*.png
//
// Why it exists here: `flutter analyze` and a passing test suite both had nothing to say about an
// app that displayed LIVE while its audio went nowhere. Neither of them can see. This is not a
// golden test — nothing fails on a changed pixel — it is a way to look.

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:earshot/main.dart';

/// The owner's phone: 1220x2712 at density 3.0.
const _size = Size(406.7, 904);
const _dpr = 3.0;
const _outDir = '/tmp/earshot-screens';

const _rootKey = ValueKey('shot-root');
const _control = MethodChannel('earshot/control');
const _events = EventChannel('earshot/events');

/// The prefs and state the platform would return. Set per scenario before pumping.
late Map<String, Object?> _prefs;
late Map<String, Object?> _network;

/// Flutter's test environment has no system fonts, so every word renders as a black box unless the
/// real ones are registered. The point of these shots is to read the wording, so they are.
///
/// Roboto is what Material asks for on Android, and it ships inside the Flutter SDK — the same file
/// the app will use on the phone.
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
        throw StateError('gen_screens: ${file.path} is missing.');
      }
      loader.addFont(Future.value(file.readAsBytesSync().buffer.asByteData()));
    }
    await loader.load();
  }

  // The panel design rests on its monospace: without it every number falls out of its column and
  // the picture is of a screen the phone will never show.
  final monoLoader = FontLoader('JetBrainsMono');
  for (final name in [
    'JetBrainsMonoNL-Regular.ttf',
    'JetBrainsMonoNL-Medium.ttf',
    'JetBrainsMonoNL-Bold.ttf',
  ]) {
    final file = File('assets/fonts/$name');
    if (!file.existsSync()) {
      throw StateError('gen_screens: ${file.path} is missing.');
    }
    monoLoader.addFont(Future.value(file.readAsBytesSync().buffer.asByteData()));
  }
  await monoLoader.load();
}

/// The SDK's own copy of Roboto and the Material icon font.
///
/// Found by walking up from the running binary rather than assumed, because what runs a widget
/// test is `flutter_tester` (deep inside `bin/cache/artifacts/engine/...`) and not the `dart` in
/// the SDK — a guess at that layout is how this harness spent an afternoon quietly loading nothing
/// and photographing black boxes. Missing fonts now throw, for the same reason.
Directory _materialFonts() {
  var dir = File(Platform.resolvedExecutable).parent;
  for (var up = 0; up < 8; up++) {
    final candidate = Directory('${dir.path}/material_fonts');
    if (candidate.existsSync()) return candidate;
    final nested = Directory('${dir.path}/artifacts/material_fonts');
    if (nested.existsSync()) return nested;
    dir = dir.parent;
  }
  throw StateError(
    'gen_screens: could not find the SDK material_fonts directory from '
    '${Platform.resolvedExecutable}. Without it every word renders as a black box, which is not '
    'a screenshot of anything.',
  );
}

void main() {
  // Before anything registers a font: FontLoader needs the binding to exist, and silently does
  // nothing useful if it does not — which is exactly how a harness ends up producing pages of
  // black boxes and calling it a screenshot.
  TestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    Directory(_outDir).createSync(recursive: true);
    await _loadFonts();
  });

  setUp(() {
    _prefs = {
      'host': '192.168.1.20',
      'port': 47811,
      'source': 7,
      'rate': 48000,
      'known': '',
    };
    _network = {'transport': 'wifi', 'ip': '192.168.1.9', 'prefix': 24};

    // The event channel's own `listen`/`cancel` calls, which are a method channel underneath and
    // otherwise throw MissingPluginException before a single event can be pushed.
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

  testWidgets('the phone is on mobile data', (tester) async {
    _network = {'transport': 'cellular', 'ip': null, 'prefix': 0};
    await _open(tester);
    await _shoot(tester, '1-blocked-mobile-data');
  });

  testWidgets('the phone is on the same Wi-Fi', (tester) async {
    await _open(tester);
    await _shoot(tester, '2-ready-same-wifi');
  });

  testWidgets('the phone is on a different network', (tester) async {
    _network = {'transport': 'wifi', 'ip': '192.168.5.9', 'prefix': 24};
    await _open(tester);
    await _shoot(tester, '3-different-network');
  });

  testWidgets('no network at all', (tester) async {
    _network = {'transport': 'none', 'ip': null, 'prefix': 0};
    await _open(tester);
    await _shoot(tester, '4-no-network');
  });

  testWidgets('streaming, and the PC is answering', (tester) async {
    await _open(tester);
    await _start(tester);
    await _stats(tester, level: 0.62, packets: 4210, answeredMsAgo: 120);
    await _shoot(tester, '5-connected');
  });

  testWidgets('streaming, and the PC has gone quiet', (tester) async {
    await _open(tester);
    await _start(tester);
    await _stats(tester, level: 0.5, packets: 900, answeredMsAgo: 5200);
    await _shoot(tester, '6-no-answer');
  });

  testWidgets('streaming and muted, with the PC still there', (tester) async {
    await _open(tester);
    await _start(tester);
    await _stats(tester, level: 0, packets: 2100, answeredMsAgo: 400);
    await _emit(tester, {'event': 'muted', 'muted': true});
    await _shoot(tester, '7-muted-but-connected');
  });

  /// The remembered list, which is what a dual-boot PC looks like: one machine, two addresses,
  /// and names so they can be told apart. A widget test has no network, so nothing answers the
  /// sweep — which is exactly the situation the remembered list was built for, and makes this the
  /// honest picture of it.
  testWidgets('two remembered PCs, one chosen', (tester) async {
    _prefs = {
      ..._prefs,
      'host': '192.168.1.20',
      'known': '[{"host":"192.168.1.20","port":47811,"name":"Windows","seen":2},'
          '{"host":"192.168.1.37","port":47811,"name":"Linux","seen":1}]',
    };
    await _open(tester);
    await _shoot(tester, '9-remembered-pcs');
  });

  testWidgets('typing one in', (tester) async {
    await _open(tester);
    await tester.tap(find.text('+ not listed? type it in'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), '192.168.1.2');
    await tester.pumpAndSettle();
    await _shoot(tester, '10-typing-it-in');
  });

  /// The microphone row, open. Two ways to hear now rather than three -- the phone's own cleaner
  /// was removed on 2026-10-26 -- plus the switch that asks the PC to do it instead.
  testWidgets('the two ways to hear, and the cleaning switch', (tester) async {
    await _open(tester);
    await tester.tap(find.text('[+]').last);
    await tester.pumpAndSettle();
    await _shoot(tester, '11-audio-open');
  });

  /// The switch turned off. The line underneath must stop claiming anything.
  testWidgets('cleaning switched off', (tester) async {
    await _open(tester);
    await tester.tap(find.text('[+]').last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('[x]  clean the noise'));
    await tester.pumpAndSettle();
    await _shoot(tester, '12-cleaning-off');
  });

  /// 🔴 The case the whole two-bit protocol exists for: the switch says yes, and the PC says it
  /// cannot. The panel must draw the PC's answer, not the switch's.
  testWidgets('the pc is too old to clean', (tester) async {
    await _open(tester);
    await tester.tap(find.text('+ not listed? type it in'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), '192.168.1.20');
    await tester.pumpAndSettle();
    await _start(tester);
    await _stats(
      tester,
      level: 0.4,
      packets: 500,
      answeredMsAgo: 20,
      pcCanClean: false,
      pcCleaning: false,
    );
    // A confirmed session collapses to the quiet screen, so the way back to the switch is the
    // way a thumb would really take it: tap the screen, then open the microphone row. This is
    // also the proof that the switch IS reachable mid-session, which is the whole point of it
    // being live.
    await tester.tapAt(const Offset(200, 300));
    await tester.pump(const Duration(milliseconds: 420));
    await tester.pump(const Duration(milliseconds: 420));
    await tester.tap(find.text('[+]').last);
    await tester.pump(const Duration(milliseconds: 420));
    await _shoot(tester, '13-pc-cannot-clean');
  });

  /// 🔴 The state that cost an evening: the PC replies to every packet and plays none of them,
  /// which is what a pre-Opus receiver does with an Opus stream. The app used to show LIVE.
  testWidgets('the pc answers but plays nothing', (tester) async {
    await _open(tester);
    await tester.tap(find.text('+ not listed? type it in'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), '192.168.1.20');
    await tester.pumpAndSettle();
    await _start(tester);
    // Six seconds of a PC answering while holding nothing at all.
    await _emit(tester, {
      'event': 'stats',
      'packets': 300,
      'bytes': 300 * 1936,
      'level': 0.4,
      'rate': 48000,
      'source': 7,
      'answeredMsAgo': 20,
      'pcBufferedMs': 0.0,
      'pcCanClean': false,
      'pcCleaning': false,
    });
    await tester.pump(const Duration(seconds: 6));
    await _emit(tester, {
      'event': 'stats',
      'packets': 600,
      'bytes': 600 * 1936,
      'level': 0.4,
      'rate': 48000,
      'source': 7,
      'answeredMsAgo': 20,
      'pcBufferedMs': 0.0,
      'pcCanClean': false,
      'pcCleaning': false,
    });
    await _shoot(tester, '14-pc-answers-but-plays-nothing');
  });

  /// His own case: Tailscale up, PC on the same Wi-Fi underneath. The ordinary state for anyone
  /// running a tailnet, so it is said in one short line rather than four — and the row folds.
  testWidgets('tailscale up, pc on the same wi-fi', (tester) async {
    _network = {'transport': 'vpn', 'ip': '100.102.242.57', 'prefix': 32,
                'localIp': '192.168.1.9', 'localPrefix': 24};
    await _open(tester);
    await tester.tap(find.text('[+]').first);
    await tester.pumpAndSettle();
    await _shoot(tester, '15-tailscale-same-wifi');
  });

  testWidgets('a VPN is up', (tester) async {
    _network = {'transport': 'vpn', 'ip': '10.2.0.3', 'prefix': 32};
    await _open(tester);
    await _shoot(tester, '8-vpn');
  });
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

Future<void> _start(WidgetTester tester) =>
    _emit(tester, {'event': 'started', 'rate': 48000, 'source': 7});

Future<void> _stats(
  WidgetTester tester, {
  required double level,
  required int packets,
  required int answeredMsAgo,
  bool pcCanClean = true,
  bool pcCleaning = true,
}) =>
    _emit(tester, {
      'event': 'stats',
      'packets': packets,
      'bytes': packets * 1936,
      'level': level,
      'rate': 48000,
      'source': 7,
      'answeredMsAgo': answeredMsAgo,
      'pcBufferedMs': 60.5,
      'pcCanClean': pcCanClean,
      'pcCleaning': pcCleaning,
    });

/// Pushes an event up the same channel the service uses, so the screen is driven exactly as a real
/// session drives it rather than by reaching into its state.
Future<void> _emit(WidgetTester tester, Map<String, Object?> event) async {
  await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
      .handlePlatformMessage(
    _events.name,
    const StandardMethodCodec().encodeSuccessEnvelope(event),
    (_) {},
  );
  // ⚠️ Not pumpAndSettle. A confirmed session now shows the collapsed screen, whose ring breathes
  // for as long as the session lasts — settling waits for an animation that never ends, and the
  // run dies on a timeout having photographed nothing.
  await tester.pump(const Duration(milliseconds: 420));
  await tester.pump(const Duration(milliseconds: 420));
}

Future<void> _shoot(WidgetTester tester, String name) async {
  final boundary = tester.renderObject<RenderRepaintBoundary>(
    find.byKey(_rootKey),
  );
  await tester.runAsync(() async {
    final image = await boundary.toImage(pixelRatio: _dpr);
    final data = await image.toByteData(format: ui.ImageByteFormat.png);
    File('$_outDir/$name.png').writeAsBytesSync(data!.buffer.asUint8List());
    image.dispose();
  });
  // ignore: avoid_print
  print('shot $_outDir/$name.png');
}
