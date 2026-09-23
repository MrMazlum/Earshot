// Earshot — Android client.
//
// One direction for now (phone mic → PC), raw PCM. No discovery and no Opus yet.
//
// The PC is identified by a nine-digit pairing code rather than an address; pairing.dart holds the
// encoding and receiver/src/pairing.rs is the other half of it. Typing an address still works,
// behind "Type an address instead", because a code cannot express every address.
//
// The mic-source picker is not a settings nicety: it is the one experiment this app exists to run.
// Only the two sources that differ by Android specification are selectable — see MicSource for why
// the rest are locked.
//
// Layout rule, learned the hard way: Android 15 forces edge-to-edge, so Flutter draws *under* the
// navigation bar. The Start button lives in a pinned bar wrapped in SafeArea, never at the end of a
// scrolling list — there it sat beneath the system buttons and could not be tapped.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'pairing.dart';
import 'reachability.dart';

void main() => runApp(const EarshotApp());

const _control = MethodChannel('earshot/control');
const _events = EventChannel('earshot/events');

/// What is known about the PC at the other end.
///
/// "Streaming" and "connected" were the same word in this app once, and they are not the same
/// thing: the phone can send perfectly well and be heard by nobody. Only [connected] means the PC
/// answered — see `Protocol.TYPE_HELLO` and protocol/README.md.
enum LinkState { idle, connecting, connected, noAnswer }

/// The PC replies once a second, so three seconds of silence is two missed replies and then some.
/// It is also how long a new session waits before it stops saying "connecting" and says the truth.
const _answerTimeout = Duration(seconds: 3);

/// Matches the icon in tools/icon/make_icons.py, so the app and its launcher icon agree.
const _seed = Color(0xFF3DDC97);
const _backdrop = Color(0xFF0B1310);

/// Android's MediaRecorder.AudioSource constants.
///
/// These are not a cosmetic toggle — the number really is handed to `AudioRecord`, and the first
/// two behave differently on every Android device by specification: VOICE_COMMUNICATION runs the
/// platform's echo and noise cancellation, MIC does not.
///
/// The other three are switched off, and the reason is honesty rather than laziness. Whether a
/// phone *honours* them is up to its vendor: VOICE_RECOGNITION and CAMCORDER are very often wired
/// straight to MIC, and UNPROCESSED silently falls back to MIC unless the device advertises
/// `PROPERTY_SUPPORT_AUDIO_SOURCE_UNPROCESSED`. Offering four choices that may all be the same
/// recording is worse than offering two that are definitely not. They come back when there is a
/// measurement on a real phone to justify them.
class MicSource {
  final int id;
  final String name;
  final String blurb;

  /// False for the ones parked until they can be told apart on a real phone.
  final bool available;

  const MicSource(this.id, this.name, this.blurb, {this.available = true});

  static const all = <MicSource>[
    MicSource(7, 'Voice call',
        'Echo and noise cancellation, like a phone call. May force 16 kHz.'),
    MicSource(1, 'Plain mic', 'Barely processed. Room and fan noise come through.'),
    MicSource(6, 'Speech', 'Often the plain mic under another name.',
        available: false),
    MicSource(9, 'Unprocessed', 'No processing, where the phone supports it.',
        available: false),
    MicSource(5, 'Camcorder', 'The video mic array. Often the plain mic.',
        available: false),
  ];

  static MicSource byId(int id) =>
      all.firstWhere((s) => s.id == id, orElse: () => all.first);

  /// Falls back to the default when a stored choice is no longer selectable.
  static int usable(int id) =>
      all.any((s) => s.id == id && s.available) ? id : all.first.id;
}

class EarshotApp extends StatelessWidget {
  const EarshotApp({super.key});

  @override
  Widget build(BuildContext context) {
    final scheme = ColorScheme.fromSeed(
      seedColor: _seed,
      brightness: Brightness.dark,
    );
    return MaterialApp(
      title: 'Earshot',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        useMaterial3: true,
        colorScheme: scheme,
        // Named rather than left to the platform default, which is this same font on every Android
        // phone. Saying it out loud is what lets test/gen_screens.dart register the real file and
        // photograph readable words instead of a page of black boxes.
        fontFamily: 'Roboto',
        scaffoldBackgroundColor: _backdrop,
        inputDecorationTheme: const InputDecorationTheme(
          border: OutlineInputBorder(),
          isDense: true,
        ),
      ),
      home: const SessionPage(),
    );
  }
}

class SessionPage extends StatefulWidget {
  const SessionPage({super.key});

  @override
  State<SessionPage> createState() => _SessionPageState();
}

class _SessionPageState extends State<SessionPage> {
  final _code = TextEditingController();
  final _host = TextEditingController();
  final _port = TextEditingController(text: '$defaultPort');

  /// True when the user has opted out of pairing codes and is typing an address. Needed for the
  /// addresses a code cannot express — anything outside the private blocks, or an unusual port.
  bool _manual = false;

  int _source = 7;
  int _rate = 48000;

  bool _running = false;

  /// The mic gate. Owned by the service, not by this screen — it can also be flipped from the
  /// notification while the app is not on screen, so it always comes back over the event channel
  /// rather than being set optimistically here.
  bool _muted = false;

  int _packets = 0;
  int _bytes = 0;
  double _level = 0;
  int _actualRate = 0;
  String? _error;

  /// What the phone is attached to. Comes from Android, changes under us, and is the reason a
  /// perfectly typed pairing code can still reach nothing.
  NetworkState _net = NetworkState.unknown;

  /// How long since the PC last answered, in milliseconds, or -1 if it never has this session.
  int _answeredMsAgo = -1;

  /// How much audio the PC said it was holding. Its own words, not a guess from here.
  double _pcBufferedMs = 0;

  /// When the current session started, so "no reply yet" can be told from "no reply, ever".
  DateTime? _startedAt;

  /// True when the microphone was refused permanently, so the error needs a way out rather than
  /// an instruction to try again.
  bool _permissionBlocked = false;

  @override
  void initState() {
    super.initState();
    _events.receiveBroadcastStream().listen(_onEvent);
    _restore();
  }

  @override
  void dispose() {
    _code.dispose();
    _host.dispose();
    _port.dispose();
    super.dispose();
  }

  /// Where Start would send audio right now, or null if the field is not usable yet.
  Destination? get _target {
    if (_manual) {
      final host = _host.text.trim();
      if (host.isEmpty) return null;
      return Destination(host, int.tryParse(_port.text.trim()) ?? defaultPort);
    }
    return resolvePairingCode(_code.text);
  }

  /// What this phone can work out about reaching the PC before a packet is sent.
  Reachability get _reach => reachability(_net, _target);

  /// What the PC has actually said, which is the only thing that can mean "connected".
  ///
  /// A session that has heard nothing is *connecting* for [_answerTimeout] and then honest about
  /// it. The distinction matters: the first second of any session has heard nothing either.
  LinkState get _link {
    if (!_running) return LinkState.idle;
    final answered = _answeredMsAgo;
    if (answered >= 0 && answered < _answerTimeout.inMilliseconds) {
      return LinkState.connected;
    }
    final started = _startedAt;
    if (answered < 0 &&
        started != null &&
        DateTime.now().difference(started) < _answerTimeout) {
      return LinkState.connecting;
    }
    return LinkState.noAnswer;
  }

  Future<void> _restore() async {
    try {
      final p = await _control.invokeMapMethod<String, dynamic>('getPrefs');
      final running = await _control.invokeMethod<bool>('isRunning') ?? false;
      final muted = await _control.invokeMethod<bool>('isMuted') ?? false;
      // Asked for rather than waited for: the network only *changes* rarely, and a screen that
      // opened to a blank verdict would be blank for as long as nothing changed.
      final net = await _control.invokeMapMethod<String, dynamic>('getNetwork');
      if (!mounted || p == null) return;
      setState(() {
        if (net != null) _net = NetworkState.fromMap(net);
        _code.text = (p['code'] as String?) ?? '';
        _manual = (p['manual'] as bool?) ?? false;
        _host.text = (p['host'] as String?) ?? '';
        _port.text = '${p['port'] ?? defaultPort}';
        // A source that has since been parked would otherwise stay selected and invisible.
        _source = MicSource.usable((p['source'] as int?) ?? 7);
        _rate = (p['rate'] as int?) ?? 48000;
        _running = running;
        _muted = running && muted;
      });
    } on PlatformException {
      // First run — defaults are fine.
    }
  }

  void _onEvent(dynamic event) {
    if (event is! Map || !mounted) return;
    setState(() {
      switch (event['event']) {
        case 'started':
          _running = true;
          _muted = false;
          _error = null;
          _actualRate = (event['rate'] as int?) ?? 0;
          _packets = 0;
          _bytes = 0;
          // A new session has heard nothing yet, whatever the last one heard.
          _answeredMsAgo = -1;
          _pcBufferedMs = 0;
          _startedAt = DateTime.now();
          break;
        case 'network':
          _net = NetworkState.fromMap(event);
          break;
        case 'muted':
          _muted = (event['muted'] as bool?) ?? false;
          if (_muted) _level = 0;
          break;
        case 'stats':
          _packets = (event['packets'] as num?)?.toInt() ?? _packets;
          _bytes = (event['bytes'] as num?)?.toInt() ?? _bytes;
          _level = (event['level'] as num?)?.toDouble() ?? 0;
          _actualRate = (event['rate'] as int?) ?? _actualRate;
          _answeredMsAgo = (event['answeredMsAgo'] as num?)?.toInt() ?? -1;
          _pcBufferedMs = (event['pcBufferedMs'] as num?)?.toDouble() ?? 0;
          break;
        case 'error':
          _error = event['message'] as String?;
          _running = false;
          _muted = false;
          break;
        case 'stopped':
          _running = false;
          _muted = false;
          _level = 0;
          _answeredMsAgo = -1;
          _startedAt = null;
          break;
      }
    });
  }

  Future<void> _toggle() async {
    if (_running) {
      await _control.invokeMethod('stop');
      return;
    }

    // Dismiss the keyboard first, or the confirmation is hidden behind it.
    FocusScope.of(context).unfocus();

    final target = _target;
    if (target == null) {
      setState(() => _error = _manual
          ? "Type your PC's address first."
          : looksLikePairingCode(_code.text)
              ? 'That is not a working pairing code. Check the nine digits '
                  'against the ones on your PC.'
              : 'Type the nine-digit pairing code your PC is showing.');
      return;
    }

    // Re-read the network at the moment of the press. The card above may have been drawn before
    // the user walked out of Wi-Fi range, and starting a session that cannot work is the whole
    // failure this is here to stop.
    await _refreshNetwork();
    final blocked = _reach;
    if (blocked.blocks) {
      setState(() => _error = blocked.detail ?? blocked.headline);
      return;
    }

    final granted =
        await _control.invokeMethod<bool>('requestPermissions') ?? false;
    if (!granted) {
      // Two refusals and Android stops showing the dialog at all, so "press Start again" would be
      // advice that can never work. Offer the settings page instead, which is the only way back.
      final state =
          await _control.invokeMethod<String>('micPermissionState') ?? 'askable';
      if (!mounted) return;
      setState(() {
        _permissionBlocked = state == 'blocked';
        _error = _permissionBlocked
            ? 'Android will not ask again, because the microphone was refused '
                'earlier. Turn it on in the app settings.'
            : 'Allow the microphone permission, then press Start again.';
      });
      return;
    }
    if (_permissionBlocked) setState(() => _permissionBlocked = false);

    final args = {
      'host': target.host,
      'port': target.port,
      'source': _source,
      'rate': _rate,
    };
    // The code and the mode are remembered but never sent to the service — it only ever deals in
    // an address and a port.
    await _control.invokeMethod('setPrefs', {
      ...args,
      'code': _code.text,
      'manual': _manual,
    });

    try {
      await _control.invokeMethod('start', args);
      setState(() => _error = null);
    } on PlatformException catch (e) {
      setState(() => _error = e.message);
    }
  }

  Future<void> _refreshNetwork() async {
    try {
      final net = await _control.invokeMapMethod<String, dynamic>('getNetwork');
      if (!mounted || net == null) return;
      setState(() => _net = NetworkState.fromMap(net));
    } on PlatformException {
      // An older build of the app half; the verdict simply stays unknown.
    }
  }

  /// Asks the service to open or close the gate. The screen does not set `_muted` itself — it
  /// waits for the service to confirm, so what is shown is what the microphone is actually doing.
  Future<void> _toggleMute() async {
    if (!_running) return;
    await _control.invokeMethod('setMuted', {'muted': !_muted});
  }

  /// The explanations that used to sit permanently on the main screen. They are worth having and
  /// worth reading once; they are not worth the space they took every time the app was opened.
  void _showMicInfo(BuildContext context) {
    showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (context) {
        final theme = Theme.of(context);
        return SafeArea(
          child: Padding(
            padding: const EdgeInsets.fromLTRB(24, 0, 24, 24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Choosing a microphone',
                    style: theme.textTheme.titleMedium
                        ?.copyWith(fontWeight: FontWeight.w600)),
                const SizedBox(height: 12),
                Text(
                  'Android phones have several microphones and a chip that removes background '
                  'noise. Which of that you get depends on the source an app asks for.',
                  style: theme.textTheme.bodyMedium,
                ),
                const SizedBox(height: 16),
                for (final s in MicSource.all)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 10),
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Icon(
                          s.available ? Icons.check_circle_outline : Icons.lock_outline,
                          size: 17,
                          color: s.available
                              ? theme.colorScheme.primary
                              : theme.hintColor,
                        ),
                        const SizedBox(width: 10),
                        Expanded(
                          child: Text.rich(TextSpan(children: [
                            TextSpan(
                              text: '${s.name}  ',
                              style: const TextStyle(fontWeight: FontWeight.w600),
                            ),
                            TextSpan(
                              text: s.blurb,
                              style: TextStyle(color: theme.hintColor),
                            ),
                          ]), style: theme.textTheme.bodySmall),
                        ),
                      ],
                    ),
                  ),
                const SizedBox(height: 6),
                Text(
                  'The locked ones come back once they can be shown to sound different on a real '
                  'phone. On many devices they are the plain microphone under another name, and a '
                  'choice that changes nothing is worse than no choice.',
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
                ),
                const SizedBox(height: 14),
                Text(
                  'Sample rate is a request, not a promise: the noise-cancelled source may hand '
                  'back 16 kHz whatever you pick. The rate you actually got is shown while '
                  'streaming.',
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
                ),
              ],
            ),
          ),
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final kbps =
        _packets > 0 ? (_bytes * 8 / 1000 / (_packets * 0.02)).round() : 0;

    return Scaffold(
      body: SafeArea(
        bottom: false,
        child: Column(
          children: [
            _Header(link: _link, muted: _muted),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
                children: [
                  _LevelMeter(
                    level: _level,
                    live: _running,
                    muted: _muted,
                    // The numbers replace the caption rather than joining it: while streaming they
                    // are the thing worth reading, and telling someone to speak is not.
                    metrics: _running && !_muted
                        ? '${(_actualRate / 1000).round()} kHz  ·  '
                            '$kbps kbps  ·  $_packets pkt'
                        : null,
                  ),
                  const SizedBox(height: 14),
                  // Above "Your PC" on purpose: it is the question asked first, and the one that
                  // used to have no answer anywhere on this screen.
                  _NetworkCard(
                    reach: _reach,
                    link: _link,
                    answeredMsAgo: _answeredMsAgo,
                    pcBufferedMs: _pcBufferedMs,
                    onOpenWifi: () => _control.invokeMethod('openWifiSettings'),
                  ),
                  const SizedBox(height: 14),
                  _Card(
                    title: 'Your PC',
                    action: _running
                        ? null
                        : _CardAction(
                            label: _manual ? 'Use a code' : 'Use an address',
                            onTap: () => setState(() => _manual = !_manual),
                          ),
                    child: _manual
                        ? _AddressFields(
                            host: _host,
                            port: _port,
                            enabled: !_running,
                            onChanged: () => setState(() {}),
                          )
                        : _CodeField(
                            controller: _code,
                            enabled: !_running,
                            onChanged: () => setState(() {}),
                          ),
                  ),
                  const SizedBox(height: 14),
                  _Card(
                    title: 'Audio',
                    // Why three of the five are locked is a real answer, but it is a paragraph, and
                    // a paragraph does not belong on the screen someone uses every day.
                    action: _CardAction(
                      icon: Icons.info_outline,
                      label: 'About',
                      onTap: () => _showMicInfo(context),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Wrap(
                          spacing: 8,
                          runSpacing: 8,
                          children: MicSource.all
                              .map((s) => ChoiceChip(
                                    label: Text(s.name),
                                    avatar: s.available
                                        ? null
                                        : const Icon(Icons.lock_outline,
                                            size: 15),
                                    selected: s.available && _source == s.id,
                                    // A locked chip stays visible and tappable so its blurb can
                                    // explain itself; it just cannot be chosen.
                                    onSelected: _running
                                        ? null
                                        : (_) => setState(() {
                                              if (s.available) _source = s.id;
                                            }),
                                    showCheckmark: s.available,
                                  ))
                              .toList(),
                        ),
                        const SizedBox(height: 10),
                        Text(
                          MicSource.byId(_source).blurb,
                          style: theme.textTheme.bodySmall
                              ?.copyWith(color: theme.hintColor),
                        ),
                        const Padding(
                          padding: EdgeInsets.symmetric(vertical: 14),
                          child: Divider(height: 1),
                        ),
                        Row(
                          children: [
                            Expanded(
                              child: Text('Sample rate',
                                  style: theme.textTheme.bodyMedium),
                            ),
                            SegmentedButton<int>(
                              segments: const [
                                ButtonSegment(
                                    value: 48000, label: Text('48 kHz')),
                                ButtonSegment(
                                    value: 16000, label: Text('16 kHz')),
                              ],
                              selected: {_rate},
                              showSelectedIcon: false,
                              style: const ButtonStyle(
                                visualDensity: VisualDensity.compact,
                                tapTargetSize:
                                    MaterialTapTargetSize.shrinkWrap,
                              ),
                              onSelectionChanged: _running
                                  ? null
                                  : (v) => setState(() => _rate = v.first),
                            ),
                          ],
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
      // Pinned, and inside a SafeArea of its own. This is the fix for the button that used to sit
      // underneath Android's navigation bar.
      bottomNavigationBar: _ActionBar(
        running: _running,
        muted: _muted,
        error: _error,
        // Greyed rather than allowed-and-then-refused: the card directly above says why, in full,
        // and a button that can be pressed into a session that cannot work is what got us here.
        canStart: !_reach.blocks,
        onPressed: _toggle,
        onMute: _toggleMute,
        onOpenSettings: _permissionBlocked
            ? () => _control.invokeMethod('openAppSettings')
            : null,
      ),
    );
  }
}

/// Keeps the pairing field looking like `123 456 789` while it is being typed, and refuses a tenth
/// digit rather than accepting one and failing later.
class _CodeFormatter extends TextInputFormatter {
  @override
  TextEditingValue formatEditUpdate(
      TextEditingValue oldValue, TextEditingValue newValue) {
    final digits =
        newValue.text.replaceAll(RegExp(r'\D'), '').characters.take(9).join();
    final groups = <String>[];
    for (var i = 0; i < digits.length; i += 3) {
      groups.add(digits.substring(i, i + 3 > digits.length ? digits.length : i + 3));
    }
    final text = groups.join(' ');
    return TextEditingValue(
      text: text,
      // Keep the caret at the end: this field is typed into, never edited in the middle.
      selection: TextSelection.collapsed(offset: text.length),
    );
  }
}

/// The pairing code, with the address it resolves to shown underneath.
///
/// Echoing the address back is what stops the code feeling like a black box — the user can see
/// that it landed somewhere plausible before pressing Start.
class _CodeField extends StatelessWidget {
  final TextEditingController controller;
  final bool enabled;
  final VoidCallback onChanged;
  const _CodeField({
    required this.controller,
    required this.enabled,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final target = resolvePairingCode(controller.text);
    final typed = controller.text.replaceAll(RegExp(r'\D'), '').length;

    final (icon, colour, message) = switch ((typed, target)) {
      (0, _) => (null, theme.hintColor, 'Nine digits, shown on your PC.'),
      (_, final t?) => (
          Icons.check_circle_outline,
          theme.colorScheme.primary,
          'Ready — ${t.host}${t.port == defaultPort ? '' : ':${t.port}'}',
        ),
      (9, _) => (
          Icons.error_outline,
          theme.colorScheme.error,
          'Not a code your PC could have shown. Check the digits.',
        ),
      _ => (null, theme.hintColor, '${9 - typed} more to go.'),
    };

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        TextField(
          controller: controller,
          enabled: enabled,
          keyboardType: TextInputType.number,
          autocorrect: false,
          inputFormatters: [_CodeFormatter()],
          onChanged: (_) => onChanged(),
          style: const TextStyle(
            fontSize: 26,
            letterSpacing: 3,
            fontWeight: FontWeight.w600,
            fontFeatures: [FontFeature.tabularFigures()],
          ),
          decoration: const InputDecoration(
            labelText: 'Pairing code',
            hintText: '000 000 000',
            contentPadding: EdgeInsets.symmetric(horizontal: 14, vertical: 16),
          ),
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            if (icon != null) ...[
              Icon(icon, size: 16, color: colour),
              const SizedBox(width: 6),
            ],
            Expanded(
              child: Text(message,
                  style: theme.textTheme.bodySmall?.copyWith(color: colour)),
            ),
          ],
        ),
      ],
    );
  }
}

/// The escape hatch: an address a pairing code cannot express.
class _AddressFields extends StatelessWidget {
  final TextEditingController host;
  final TextEditingController port;
  final bool enabled;
  final VoidCallback onChanged;
  const _AddressFields({
    required this.host,
    required this.port,
    required this.enabled,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          flex: 3,
          child: TextField(
            controller: host,
            enabled: enabled,
            keyboardType: TextInputType.url,
            autocorrect: false,
            onChanged: (_) => onChanged(),
            decoration: const InputDecoration(
              labelText: 'Address',
              hintText: '192.168.1.20',
            ),
          ),
        ),
        const SizedBox(width: 10),
        Expanded(
          flex: 2,
          child: TextField(
            controller: port,
            enabled: enabled,
            keyboardType: TextInputType.number,
            onChanged: (_) => onChanged(),
            decoration: const InputDecoration(labelText: 'Port'),
          ),
        ),
      ],
    );
  }
}

/// Amber, for the one state that is neither running nor stopped. Deliberately not the primary
/// colour: muted must never be mistakable for live at a glance.
const _mutedColour = Colors.amber;

/// The network card: where this phone is, whether the PC can be reached from there, and — once
/// streaming — whether the PC is actually answering.
///
/// It sits above everything else because it is the first question, and because for one whole
/// session it was a question this screen could not answer at all.
class _NetworkCard extends StatelessWidget {
  final Reachability reach;
  final LinkState link;
  final int answeredMsAgo;
  final double pcBufferedMs;
  final VoidCallback onOpenWifi;

  const _NetworkCard({
    required this.reach,
    required this.link,
    required this.answeredMsAgo,
    required this.pcBufferedMs,
    required this.onOpenWifi,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    if (reach.verdict == Reach.unknown && reach.headline.isEmpty) {
      return const SizedBox.shrink();
    }

    final (icon, colour) = switch (reach.verdict) {
      Reach.blocked => (Icons.error_outline, theme.colorScheme.error),
      Reach.warn => (Icons.warning_amber_rounded, _mutedColour),
      Reach.ok => (Icons.check_circle_outline, theme.colorScheme.primary),
      Reach.unknown => (Icons.wifi_rounded, theme.hintColor),
    };

    return _Card(
      title: 'Network',
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Icon(icon, size: 17, color: colour),
              const SizedBox(width: 10),
              Expanded(
                child: Text(
                  reach.headline,
                  style: theme.textTheme.bodyMedium
                      ?.copyWith(color: colour, fontWeight: FontWeight.w600),
                ),
              ),
            ],
          ),
          if (reach.detail != null) ...[
            const SizedBox(height: 6),
            Padding(
              padding: const EdgeInsets.only(left: 27),
              child: Text(
                reach.detail!,
                style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
              ),
            ),
          ],
          if (reach.offerWifi) ...[
            const SizedBox(height: 4),
            Padding(
              padding: const EdgeInsets.only(left: 19),
              child: TextButton.icon(
                onPressed: onOpenWifi,
                icon: const Icon(Icons.wifi_rounded, size: 18),
                label: const Text('Open Wi-Fi settings'),
                style: TextButton.styleFrom(
                  foregroundColor: colour,
                  padding: const EdgeInsets.symmetric(horizontal: 8),
                  visualDensity: VisualDensity.compact,
                ),
              ),
            ),
          ],
          // The second half of the answer, and the only one that is evidence rather than
          // reasoning: what the PC itself has said, and how long ago it said it.
          if (link != LinkState.idle) ...[
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 12),
              child: Divider(height: 1),
            ),
            _PcReply(
              link: link,
              answeredMsAgo: answeredMsAgo,
              pcBufferedMs: pcBufferedMs,
            ),
          ],
        ],
      ),
    );
  }
}

/// What the PC is saying back, in a sentence rather than a number.
class _PcReply extends StatelessWidget {
  final LinkState link;
  final int answeredMsAgo;
  final double pcBufferedMs;

  const _PcReply({
    required this.link,
    required this.answeredMsAgo,
    required this.pcBufferedMs,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);

    final (icon, colour, headline, detail) = switch (link) {
      LinkState.connected => (
          Icons.check_circle_outline,
          theme.colorScheme.primary,
          'Your PC is hearing this',
          'It answered ${_ago(answeredMsAgo)} ago'
              '${pcBufferedMs > 0 ? ' · holding ${pcBufferedMs.toStringAsFixed(0)} ms' : ''}.',
        ),
      LinkState.connecting => (
          Icons.hourglass_empty_rounded,
          theme.hintColor,
          'Waiting for your PC to answer',
          null,
        ),
      LinkState.noAnswer => (
          Icons.error_outline,
          theme.colorScheme.error,
          'Your PC is not answering',
          'The microphone is being sent, but nothing is coming back. Check that Earshot is '
              'running on the PC, and that this is the right pairing code. An Earshot on the PC '
              'older than this app never answers — the sound still works, but this cannot tell.',
        ),
      LinkState.idle => (Icons.circle_outlined, theme.hintColor, '', null),
    };

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(icon, size: 17, color: colour),
            const SizedBox(width: 10),
            Expanded(
              child: Text(
                headline,
                style: theme.textTheme.bodyMedium
                    ?.copyWith(color: colour, fontWeight: FontWeight.w600),
              ),
            ),
          ],
        ),
        if (detail != null) ...[
          const SizedBox(height: 6),
          Padding(
            padding: const EdgeInsets.only(left: 27),
            child: Text(
              detail,
              style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
            ),
          ),
        ],
      ],
    );
  }

  /// Sub-second ages read as "a moment": nobody needs 240 ms, and a number that fast only draws
  /// the eye to a figure that is changing five times a second.
  static String _ago(int ms) {
    if (ms < 0) return 'never';
    if (ms < 1000) return 'a moment';
    return '${(ms / 1000).toStringAsFixed(0)} s';
  }
}

class _Header extends StatelessWidget {
  final LinkState link;
  final bool muted;
  const _Header({required this.link, required this.muted});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
      child: Row(
        children: [
          Image.asset('assets/icon.png', width: 34, height: 34),
          const SizedBox(width: 10),
          Text(
            'Earshot',
            style: theme.textTheme.titleLarge
                ?.copyWith(fontWeight: FontWeight.w600),
          ),
          const Spacer(),
          _LiveBadge(link: link, muted: muted),
        ],
      ),
    );
  }
}

/// The word at the top of the screen.
///
/// It used to read LIVE the moment the microphone opened, which was true about the phone and told
/// the user nothing about the PC — and on a session that reached nobody it was the most reassuring
/// thing on the screen. Now it names the link: the microphone being open is the *least* of it.
class _LiveBadge extends StatelessWidget {
  final LinkState link;
  final bool muted;
  const _LiveBadge({required this.link, required this.muted});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    // Mute wins the badge while connected: it is the state most likely to be misread across a
    // room, and its cost — talking to nobody — is the one this badge exists to prevent.
    final (colour, label) = switch ((link, muted)) {
      (LinkState.idle, _) => (theme.disabledColor, 'IDLE'),
      (LinkState.noAnswer, _) => (theme.colorScheme.error, 'NO ANSWER'),
      (LinkState.connecting, _) => (theme.hintColor, 'CONNECTING'),
      (LinkState.connected, true) => (_mutedColour, 'MUTED'),
      (LinkState.connected, false) => (theme.colorScheme.primary, 'CONNECTED'),
    };
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 5),
      decoration: BoxDecoration(
        color: colour.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(20),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(color: colour, shape: BoxShape.circle),
          ),
          const SizedBox(width: 7),
          Text(
            label,
            style: TextStyle(
              color: colour,
              fontSize: 11,
              fontWeight: FontWeight.bold,
              letterSpacing: 1.1,
            ),
          ),
        ],
      ),
    );
  }
}

/// A segmented bar, because a single sliding bar makes it hard to tell a quiet signal from none at
/// all — and telling those apart is exactly what someone checks this screen for.
class _LevelMeter extends StatelessWidget {
  final double level;
  final bool live;
  final bool muted;

  /// Shown instead of the caption while streaming. Null when idle or muted.
  final String? metrics;
  const _LevelMeter({
    required this.level,
    required this.live,
    required this.muted,
    this.metrics,
  });

  static const _segments = 26;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    // Muted forces the meter flat rather than relying on the level happening to be zero. A meter
    // that still moved while muted would make a mute bug look like normal operation.
    final lit = muted ? 0 : (level.clamp(0.0, 1.0) * _segments).round();

    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 16),
      decoration: BoxDecoration(
        color: theme.colorScheme.surfaceContainerHighest.withValues(alpha: 0.28),
        borderRadius: BorderRadius.circular(16),
        border: Border.all(
          color: theme.colorScheme.onSurface.withValues(alpha: 0.06),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            height: 42,
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.end,
              children: List.generate(_segments, (i) {
                final on = live && i < lit;
                // Taller towards the right, so the meter has a direction even at rest.
                final height = 12.0 + (i / (_segments - 1)) * 30.0;
                return Expanded(
                  child: Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 1.5),
                    child: AnimatedContainer(
                      duration: const Duration(milliseconds: 90),
                      height: height,
                      decoration: BoxDecoration(
                        color: on
                            ? _colourFor(i, theme)
                            : theme.colorScheme.onSurface
                                .withValues(alpha: 0.10),
                        borderRadius: BorderRadius.circular(2),
                      ),
                    ),
                  ),
                );
              }),
            ),
          ),
          const SizedBox(height: 12),
          Text(
            metrics ?? (muted ? 'Muted — nothing is being sent' : 'Not streaming'),
            style: theme.textTheme.bodySmall?.copyWith(
              color: muted ? _mutedColour : theme.hintColor,
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
        ],
      ),
    );
  }

  Color _colourFor(int i, ThemeData theme) {
    final t = i / (_segments - 1);
    if (t > 0.92) return theme.colorScheme.error;
    if (t > 0.78) return Colors.amber;
    return theme.colorScheme.primary;
  }
}

/// A secondary control in a card header: the escape hatch, the explanation. Small on purpose - it
/// is there when looked for and quiet when not.
class _CardAction {
  final String label;
  final IconData? icon;
  final VoidCallback onTap;
  const _CardAction({required this.label, this.icon, required this.onTap});
}

/// One group of controls. Structure comes from the container and the header, so the content does
/// not have to be introduced by a paragraph of text.
class _Card extends StatelessWidget {
  final String title;
  final _CardAction? action;
  final Widget child;
  const _Card({required this.title, this.action, required this.child});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 16),
      decoration: BoxDecoration(
        color: theme.colorScheme.surfaceContainerHighest.withValues(alpha: 0.28),
        borderRadius: BorderRadius.circular(16),
        border: Border.all(
          color: theme.colorScheme.onSurface.withValues(alpha: 0.06),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            height: 28,
            child: Row(
              children: [
                Text(
                  title.toUpperCase(),
                  style: theme.textTheme.labelSmall?.copyWith(
                    color: theme.colorScheme.primary,
                    letterSpacing: 1.2,
                    fontWeight: FontWeight.w700,
                  ),
                ),
                const Spacer(),
                if (action != null)
                  TextButton.icon(
                    onPressed: action!.onTap,
                    icon: action!.icon == null
                        ? const SizedBox.shrink()
                        : Icon(action!.icon, size: 16),
                    label: Text(action!.label),
                    style: TextButton.styleFrom(
                      foregroundColor: theme.hintColor,
                      textStyle: theme.textTheme.labelMedium,
                      padding: const EdgeInsets.symmetric(horizontal: 8),
                      minimumSize: Size.zero,
                      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                      visualDensity: VisualDensity.compact,
                    ),
                  ),
              ],
            ),
          ),
          const SizedBox(height: 10),
          child,
        ],
      ),
    );
  }
}

/// The pinned bottom bar: the error, then the one button that matters.
class _ActionBar extends StatelessWidget {
  final bool running;
  final bool muted;
  final String? error;

  /// False when the network makes a session impossible — see [Reachability.blocks].
  final bool canStart;
  final VoidCallback onPressed;
  final VoidCallback onMute;

  /// Only set when the error is one the user cannot clear from inside the app.
  final VoidCallback? onOpenSettings;
  const _ActionBar({
    required this.running,
    required this.muted,
    required this.error,
    required this.canStart,
    required this.onPressed,
    required this.onMute,
    this.onOpenSettings,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      decoration: BoxDecoration(
        color: theme.scaffoldBackgroundColor,
        border: Border(
          top: BorderSide(
            color: theme.colorScheme.onSurface.withValues(alpha: 0.08),
          ),
        ),
      ),
      // `top: false` because the bar is already at the bottom; the bottom inset is the one that
      // matters, and it is what keeps the button clear of the navigation bar.
      child: SafeArea(
        top: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 12),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (error != null) ...[
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: theme.colorScheme.errorContainer,
                    borderRadius: BorderRadius.circular(10),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        error!,
                        style: TextStyle(
                            color: theme.colorScheme.onErrorContainer),
                      ),
                      if (onOpenSettings != null) ...[
                        const SizedBox(height: 4),
                        TextButton.icon(
                          onPressed: onOpenSettings,
                          icon: const Icon(Icons.settings_outlined, size: 18),
                          label: const Text('Open app settings'),
                          style: TextButton.styleFrom(
                            foregroundColor: theme.colorScheme.onErrorContainer,
                            padding: EdgeInsets.zero,
                            visualDensity: VisualDensity.compact,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
                const SizedBox(height: 12),
              ],
              // Idle, there is one thing to do. Live, mute is the button that gets pressed
              // constantly and Stop is the one pressed once — so mute takes the width and the
              // emphasis, and Stop is set apart where it cannot be hit by mistake.
              if (!running)
                FilledButton.icon(
                  onPressed: canStart ? onPressed : null,
                  icon: const Icon(Icons.mic_rounded),
                  style: FilledButton.styleFrom(
                    minimumSize: const Size.fromHeight(56),
                    // From the theme, not a bare TextStyle: a button's textStyle replaces the
                    // theme's outright rather than merging with it, so a hard-coded one silently
                    // drops the font family with it.
                    textStyle: theme.textTheme.titleMedium
                        ?.copyWith(fontWeight: FontWeight.w600),
                  ),
                  label: const Text('Start streaming'),
                )
              else
                Row(
                  children: [
                    Expanded(
                      flex: 3,
                      child: FilledButton.icon(
                        onPressed: onMute,
                        icon: Icon(
                          muted ? Icons.mic_off_rounded : Icons.mic_rounded,
                        ),
                        style: FilledButton.styleFrom(
                          minimumSize: const Size.fromHeight(56),
                          // Solid amber while muted: the state has to be readable across the room,
                          // because the cost of misreading it is talking to nobody.
                          backgroundColor: muted
                              ? _mutedColour
                              : theme.colorScheme.surfaceContainerHighest,
                          foregroundColor:
                              muted ? Colors.black : theme.colorScheme.onSurface,
                          textStyle: theme.textTheme.titleMedium
                              ?.copyWith(fontWeight: FontWeight.w600),
                        ),
                        label: Text(muted ? 'Unmute' : 'Mute'),
                      ),
                    ),
                    const SizedBox(width: 10),
                    Expanded(
                      flex: 2,
                      child: OutlinedButton.icon(
                        onPressed: onPressed,
                        icon: const Icon(Icons.stop_rounded),
                        style: OutlinedButton.styleFrom(
                          minimumSize: const Size.fromHeight(56),
                          foregroundColor: theme.colorScheme.error,
                          side: BorderSide(
                            color: theme.colorScheme.error.withValues(alpha: 0.5),
                          ),
                          textStyle: theme.textTheme.titleMedium
                              ?.copyWith(fontWeight: FontWeight.w600),
                        ),
                        label: const Text('Stop'),
                      ),
                    ),
                  ],
                ),
            ],
          ),
        ),
      ),
    );
  }
}

