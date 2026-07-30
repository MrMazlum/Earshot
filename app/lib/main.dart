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

void main() => runApp(const EarshotApp());

const _control = MethodChannel('earshot/control');
const _events = EventChannel('earshot/events');

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
        "Runs the phone's echo and noise cancellation, like a real call. May force 16 kHz."),
    MicSource(1, 'Plain mic',
        'Main microphone, little processing. Fan and room noise come through.'),
    MicSource(6, 'Speech',
        'Tuned for speech recognition. On many phones this is the plain microphone under another name.',
        available: false),
    MicSource(9, 'Unprocessed',
        'No vendor processing at all — but only on phones that support it, and the rest quietly give you the plain microphone instead.',
        available: false),
    MicSource(5, 'Camcorder',
        'The microphone array used for video. Also aliased to the plain microphone on a lot of devices.',
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
  int _packets = 0;
  int _bytes = 0;
  double _level = 0;
  int _actualRate = 0;
  String? _error;

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

  Future<void> _restore() async {
    try {
      final p = await _control.invokeMapMethod<String, dynamic>('getPrefs');
      final running = await _control.invokeMethod<bool>('isRunning') ?? false;
      if (!mounted || p == null) return;
      setState(() {
        _code.text = (p['code'] as String?) ?? '';
        _manual = (p['manual'] as bool?) ?? false;
        _host.text = (p['host'] as String?) ?? '';
        _port.text = '${p['port'] ?? defaultPort}';
        // A source that has since been parked would otherwise stay selected and invisible.
        _source = MicSource.usable((p['source'] as int?) ?? 7);
        _rate = (p['rate'] as int?) ?? 48000;
        _running = running;
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
          _error = null;
          _actualRate = (event['rate'] as int?) ?? 0;
          _packets = 0;
          _bytes = 0;
          break;
        case 'stats':
          _packets = (event['packets'] as num?)?.toInt() ?? _packets;
          _bytes = (event['bytes'] as num?)?.toInt() ?? _bytes;
          _level = (event['level'] as num?)?.toDouble() ?? 0;
          _actualRate = (event['rate'] as int?) ?? _actualRate;
          break;
        case 'error':
          _error = event['message'] as String?;
          _running = false;
          break;
        case 'stopped':
          _running = false;
          _level = 0;
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
            _Header(live: _running),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
                children: [
                  _LevelMeter(level: _level, live: _running),
                  const SizedBox(height: 20),
                  _Section(
                    title: 'Your PC',
                    subtitle: _manual
                        ? 'The receiver prints this when it starts.'
                        : 'Your PC shows a nine-digit code when the receiver '
                            'starts. Type it here.',
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
                  Align(
                    alignment: Alignment.centerLeft,
                    child: TextButton(
                      onPressed: _running
                          ? null
                          : () => setState(() => _manual = !_manual),
                      child: Text(_manual
                          ? 'Use a pairing code instead'
                          : 'Type an address instead'),
                    ),
                  ),
                  const SizedBox(height: 20),
                  _Section(
                    title: 'Microphone',
                    subtitle:
                        "These two run different amounts of your phone's noise "
                        'cancellation. Try both and listen.',
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
                        const SizedBox(height: 8),
                        Text(
                          'The locked ones are coming back once they can be '
                          'proven to sound different on a real phone — on many '
                          'devices they are the plain microphone under another '
                          'name, and a choice that changes nothing is worse '
                          'than no choice.',
                          style: theme.textTheme.bodySmall?.copyWith(
                            color: theme.hintColor,
                            fontStyle: FontStyle.italic,
                          ),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(height: 20),
                  _Section(
                    title: 'Sample rate',
                    subtitle:
                        'Some sources ignore this and give you 16 kHz anyway — '
                        'the real rate is shown once streaming.',
                    child: SegmentedButton<int>(
                      segments: const [
                        ButtonSegment(value: 48000, label: Text('48 kHz')),
                        ButtonSegment(value: 16000, label: Text('16 kHz')),
                      ],
                      selected: {_rate},
                      showSelectedIcon: false,
                      onSelectionChanged: _running
                          ? null
                          : (v) => setState(() => _rate = v.first),
                    ),
                  ),
                  if (_running) ...[
                    const SizedBox(height: 24),
                    _StatRow(
                        label: 'Actual sample rate', value: '$_actualRate Hz'),
                    _StatRow(label: 'Packets sent', value: '$_packets'),
                    _StatRow(label: 'Bitrate', value: '$kbps kbps'),
                    const SizedBox(height: 8),
                    Text(
                      'Raw audio for now — Opus compression comes next and cuts '
                      'this to about 32 kbps.',
                      style: theme.textTheme.bodySmall
                          ?.copyWith(color: theme.hintColor),
                    ),
                  ],
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
        error: _error,
        onPressed: _toggle,
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

class _Header extends StatelessWidget {
  final bool live;
  const _Header({required this.live});

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
          _LiveBadge(live: live),
        ],
      ),
    );
  }
}

class _LiveBadge extends StatelessWidget {
  final bool live;
  const _LiveBadge({required this.live});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colour = live ? theme.colorScheme.primary : theme.disabledColor;
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
            live ? 'LIVE' : 'IDLE',
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
  const _LevelMeter({required this.level, required this.live});

  static const _segments = 26;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final lit = (level.clamp(0.0, 1.0) * _segments).round();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Container(
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 16),
          decoration: BoxDecoration(
            color: theme.colorScheme.surfaceContainerHighest
                .withValues(alpha: 0.35),
            borderRadius: BorderRadius.circular(14),
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
                live
                    ? 'Speak — these bars should move'
                    : 'Not streaming',
                style: theme.textTheme.bodySmall
                    ?.copyWith(color: theme.hintColor),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Color _colourFor(int i, ThemeData theme) {
    final t = i / (_segments - 1);
    if (t > 0.92) return theme.colorScheme.error;
    if (t > 0.78) return Colors.amber;
    return theme.colorScheme.primary;
  }
}

class _Section extends StatelessWidget {
  final String title;
  final String? subtitle;
  final Widget child;
  const _Section({required this.title, this.subtitle, required this.child});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          title.toUpperCase(),
          style: theme.textTheme.labelSmall?.copyWith(
            color: theme.colorScheme.primary,
            letterSpacing: 1.1,
            fontWeight: FontWeight.w700,
          ),
        ),
        if (subtitle != null) ...[
          const SizedBox(height: 4),
          Text(
            subtitle!,
            style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
          ),
        ],
        const SizedBox(height: 10),
        child,
      ],
    );
  }
}

/// The pinned bottom bar: the error, then the one button that matters.
class _ActionBar extends StatelessWidget {
  final bool running;
  final String? error;
  final VoidCallback onPressed;

  /// Only set when the error is one the user cannot clear from inside the app.
  final VoidCallback? onOpenSettings;
  const _ActionBar({
    required this.running,
    required this.error,
    required this.onPressed,
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
              FilledButton.icon(
                onPressed: onPressed,
                icon: Icon(running ? Icons.stop_rounded : Icons.mic_rounded),
                style: FilledButton.styleFrom(
                  minimumSize: const Size.fromHeight(56),
                  backgroundColor: running ? theme.colorScheme.error : null,
                  foregroundColor: running ? theme.colorScheme.onError : null,
                  textStyle: const TextStyle(
                    fontSize: 16,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                label: Text(running ? 'Stop' : 'Start streaming'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _StatRow extends StatelessWidget {
  final String label;
  final String value;
  const _StatRow({required this.label, required this.value});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          Text(label, style: Theme.of(context).textTheme.bodyMedium),
          Text(
            value,
            style: const TextStyle(
              fontFeatures: [FontFeature.tabularFigures()],
            ),
          ),
        ],
      ),
    );
  }
}
