// Earshot — Android client.
//
// One direction for now (phone mic → PC), compressed with Opus.
//
// The PC is identified by its address, and in the ordinary case nobody types one: the phone knocks
// on every door on its own subnet and lists whoever answers, because the receiver already replies
// to any packet it understands. discover.dart has the mechanism. Whatever has answered before is
// remembered (known_pcs.dart), so a PC that is switched off, or on a network the sweep cannot
// reach, is still one tap away. One box takes a typed entry in any of its three forms — see
// address_entry.dart — and there is no longer a separate port field or a switch between kinds.
//
// The mic-source picker is not a settings nicety: it is the one experiment this app exists to run.
// Only the two sources that differ by Android specification are selectable — see MicSource for why
// the rest are locked.
//
// Layout rule, learned the hard way: Android 15 forces edge-to-edge, so Flutter draws *under* the
// navigation bar. The Start button lives in a pinned bar wrapped in SafeArea, never at the end of a
// scrolling list — there it sat beneath the system buttons and could not be tapped.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'address_entry.dart';
import 'collapsed_view.dart';
import 'discover.dart';
import 'known_pcs.dart';
import 'link_state.dart';
import 'reachability.dart';
import 'setup_panel.dart';
import 'theme.dart';

void main() => runApp(const EarshotApp());

/// What the app calls itself at the foot of the screen.
///
/// ⚠️ Kept in step with `pubspec.yaml` by `test/version_test.dart`, because a version string that
/// has to be edited in two places is a version string that is wrong in one of them.
const appVersion = '1.0.0';

/// What the rename dialog returns when its Forget button was the one pressed. A name nobody can
/// type, because every other string that comes back is a name.
const _forgetSentinel = '\u0000forget';

const _control = MethodChannel('earshot/control');
const _events = EventChannel('earshot/events');

/// What is known about the PC at the other end.
///
/// "Streaming" and "connected" were the same word in this app once, and they are not the same
/// thing: the phone can send perfectly well and be heard by nobody. Only [connected] means the PC
/// answered — see `Protocol.TYPE_HELLO` and protocol/README.md.
// LinkState moved to link_state.dart so the screen can be built without importing this file.

/// The PC replies once a second, so three seconds of silence is two missed replies and then some.
/// It is also how long a new session waits before it stops saying "connecting" and says the truth.
const _answerTimeout = Duration(seconds: 3);

/// How many packets may go to a PC that answers while holding no audio at all, before that is
/// called what it is.
///
/// Counted in packets rather than seconds on a clock, and deliberately: this is a statement about
/// audio that was actually sent, not about time that happened to pass. A stream is 50 packets a
/// second, so this is about five seconds of talking -- long enough that a buffer filling or a slow
/// start cannot trip it, short enough to answer "why can nobody hear me?" before it is asked
/// twice.
const _silentPcAfterPackets = 250;


/// Android's MediaRecorder.AudioSource constants.
///
/// These are not a cosmetic toggle — the number really is handed to `AudioRecord`, and the two
/// behave differently on every Android device by specification: VOICE_COMMUNICATION runs the
/// platform's echo and noise cancellation, MIC does not.
///
/// The other three are switched off, and the reason is honesty rather than laziness. Whether a
/// phone *honours* them is up to its vendor: VOICE_RECOGNITION and CAMCORDER are very often wired
/// straight to MIC, and UNPROCESSED silently falls back to MIC unless the device advertises
/// `PROPERTY_SUPPORT_AUDIO_SOURCE_UNPROCESSED`. Offering four choices that may all be the same
/// recording is worse than offering two that are definitely not. They come back when there is a
/// measurement on a real phone to justify them.
class MicSource {
  /// Android's `MediaRecorder.AudioSource` value this choice captures with.
  final int id;

  final String name;
  final String blurb;

  const MicSource(this.id, this.name, this.blurb);

  /// Two choices, and they are two because they sound different — not because Android has two
  /// microphones. Both are Android's own, and they differ by its specification: the voice-call
  /// chain runs the phone's echo and noise cancellation, the plain microphone does not.
  ///
  /// ⚠️ There was a third — the plain microphone cleaned on the phone by RNNoise. It was removed
  /// on 2026-10-26, the day after it was first heard: a keyboard and mouse clicks still came
  /// through it, because RNNoise is a model built for steady noise. Cleaning now happens on the
  /// PC, which has the processor for a model that handles transients, and the phone asks for it
  /// with a flag instead of doing it. See protocol/README.md.
  static const all = <MicSource>[
    MicSource(7, 'Voice call',
        'The phone call chain: its own echo and noise cancellation. May force 16 kHz.'),
    MicSource(1, 'Plain mic',
        'Barely processed, full band. The PC takes the noise out.'),
  ];

  /// How a choice is stored and restored. The Android source number is enough on its own now that
  /// no two choices share one.
  int get key => id;

  static MicSource byKey(int key) =>
      all.firstWhere((s) => s.key == key, orElse: () => all.first);

  /// Falls back to the default when a stored choice no longer exists.
  ///
  /// ⚠️ A phone that stored the removed third choice has `-1` saved, which matches nothing and
  /// lands on the first entry. That is the right answer and it is why this is not an assert: an
  /// app that crashed on its own old setting would be worse than one that forgets it.
  static MicSource usable(int key, int rate) => byKey(key);
}

class EarshotApp extends StatelessWidget {
  /// The look. A parameter rather than a constant so the three finished palettes can be rendered
  /// side by side for a decision, and so switching the app's look later is one line.
  final Palette palette;

  const EarshotApp({super.key, this.palette = defaultPalette});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Earshot',
      debugShowCheckedModeBanner: false,
      // One palette, named, in theme.dart — rather than a seed colour here and five hard-coded
      // greens scattered through the widgets. Swapping the look is swapping this constant.
      theme: palette.theme,
      home: SessionPage(palette: palette),
    );
  }
}

class SessionPage extends StatefulWidget {
  final Palette palette;
  const SessionPage({super.key, this.palette = defaultPalette});

  @override
  State<SessionPage> createState() => _SessionPageState();
}

class _SessionPageState extends State<SessionPage> {
  /// Where Start will send audio. Set by tapping a PC or by typing one in, and never derived from
  /// a text field halfway through being edited — a half-typed address is not a destination.
  Destination? _target;

  /// PCs that answered the last sweep, fastest first.
  List<Found> _found = const [];

  /// The ones that have answered before. Loaded from preferences, saved whenever it changes.
  KnownPcs _known = KnownPcs.empty;

  /// True while a sweep is in flight, so the row can say so rather than looking empty.
  bool _searching = false;

  StreamSubscription<Found>? _sweep;

  /// The one box. Takes an address, an address:port, or the nine-digit code — see
  /// address_entry.dart. There is no second field and no switch: his instruction, 2026-10-26.
  final _entry = TextEditingController();

  /// What is wrong with what is in the box, in the words shown under it. Null while it is empty
  /// or usable.
  String? _entryMessage;

  /// True while the "type it in" line is open. Closed is the normal state: the phone finds the
  /// PC, and a field nobody needs should not be on screen.
  bool _manualOpen = false;

  /// True while the audio card is showing all three choices rather than only the active one.
  ///
  /// His complaint, 2026-10-26: *"Audio part is consuming too much space."* Shut, the card is one
  /// line that says what the microphone is doing; open, it is the three choices and the rate.
  bool _audioOpen = false;

  /// True while the full screen is being shown on top of a confirmed session, because the user
  /// asked for it. Reset the moment the session ends, so the next one starts collapsed again.
  bool _expanded = false;

  /// The chosen microphone, as [MicSource.key].
  int _source = 7;

  /// Whether the PC is being asked to clean the voice. On by default: the phone has no cleaner of
  /// its own, so this is the only one there is.
  bool _clean = true;

  /// What the PC says, which is not the same thing as what [_clean] asks for. A receiver too old
  /// to clean answers no to both, and the panel must show that instead of the wish.
  bool _pcCanClean = false;
  bool _pcCleaning = false;

  /// Whether the PC has *ever* said it was holding audio this session.
  ///
  /// One frame of evidence is enough and it is never taken back: a buffer that has drained to
  /// nothing during a quiet moment is not the same as one that was never filled, and only the
  /// second of those means the PC is throwing the audio away.
  bool _pcEverBuffered = false;

  /// Whether the network row is unfolded. Starts folded: on a good day it says the same thing
  /// every time, and trouble forces it open on its own.
  bool _networkOpen = false;
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
  /// correctly chosen PC can still be unreachable.
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
    _sweep?.cancel();
    _entry.dispose();
    super.dispose();
  }

  /// The list the screen draws: everything answering now, then everything remembered that is not.
  List<PcRow> get _rows => mergePcs(_found, _known);

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
      // 🔴 Answering is not playing. A receiver replies to any packet it can PARSE, including
      // audio it then throws away -- which is exactly what a pre-Opus receiver does with an Opus
      // stream, and it is why this app once showed LIVE through an entirely silent session.
      //
      // The reply says how much audio the PC is holding. A receiver that is playing has a jitter
      // buffer with something in it; one that drops every frame reports zero for ever while
      // packets pour in. So: answering, not muted, hundreds of packets sent, and still nothing
      // held -- that is a PC that is not playing you.
      if (!_muted && !_pcEverBuffered && _packets > _silentPcAfterPackets) {
        return LinkState.notPlaying;
      }
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
        _known = KnownPcs.decode(p['known'] as String?);
        final host = (p['host'] as String?) ?? '';
        _target = host.isEmpty
            ? null
            : Destination(host, (p['port'] as int?) ?? defaultPort);
        if (_target != null) _entry.text = writeDestination(_target!);
        // A source that has since been parked would otherwise stay selected and invisible.
        _rate = (p['rate'] as int?) ?? 48000;
        _source = MicSource.usable((p['source'] as int?) ?? 7, _rate).key;
        _clean = (p['clean'] as bool?) ?? true;
        _running = running;
        _muted = running && muted;
      });
      // Nobody opened this screen to press a search button. A session that is already running is
      // left alone: its PC is, by definition, found.
      if (!_running) _search();
    } on PlatformException {
      // First run — defaults are fine.
    }
  }

  /// Knocks on every address on this subnet and lists whoever answers.
  ///
  /// Results are added as they arrive rather than at the end, so the PC across the desk appears
  /// immediately instead of after the timeout.
  void _search() {
    if (_net.localIp == null) {
      // The network has not reported in yet. Ask, and come back when it has.
      _refreshNetwork().then((_) {
        if (mounted && _net.localIp != null && !_searching) _search();
      });
      return;
    }
    _sweep?.cancel();
    setState(() {
      _searching = true;
      _found = const [];
    });
    _sweep = findReceivers(
      // The local network, not the active one: with a VPN up those differ, and the PC is only
      // ever on the local one.
      selfIp: _net.localIp,
      prefix: _net.localPrefix,
      // Always the usual port: a sweep asks 254 machines the same question, and a machine told to
      // listen somewhere unusual is exactly the case the typed box exists for.
      port: defaultPort,
    ).listen(
      (found) {
        if (!mounted) return;
        setState(() {
          _found = [..._found, found];
          // One PC and nothing already chosen: choose it. A second one makes it a decision, and
          // a decision is the user's.
          if (_found.length == 1 && _target == null) {
            _target = found.destination;
            _entry.text = writeDestination(found.destination);
          }
        });
      },
      onDone: () {
        if (!mounted) return;
        setState(() {
          _searching = false;
          // Nothing answered, nothing chosen, nothing remembered: offer this subnet, so what is
          // left to type is one number rather than four.
          if (_found.isEmpty && _target == null && _known.isEmpty) {
            final own = _net.localIp;
            if (own != null && own.split('.').length == 4) {
              final parts = own.split('.');
              _entry.text = '${parts[0]}.${parts[1]}.${parts[2]}.';
              _manualOpen = true;
            }
          }
          // Everything remembered that did not answer stays on the list, so a PC that is simply
          // switched off is still one tap away.
          if (_found.isEmpty && _target == null && _known.all.isNotEmpty) {
            _target = _known.all.first.destination;
            _entry.text = writeDestination(_target!);
          }
        });
      },
      cancelOnError: false,
    );
  }

  /// Chooses a PC — one that answered, or one that is remembered.
  void _pick(Destination destination) {
    setState(() {
      _target = destination;
      _entry.text = writeDestination(destination);
      _entryMessage = null;
      _error = null;
    });
  }

  /// Reads the one box on every keystroke. An empty box clears the choice rather than keeping a
  /// destination nobody can see any more.
  void _onEntryChanged() {
    final read = readDestination(_entry.text);
    setState(() {
      _entryMessage = read?.message;
      if (read != null && read.isOk) {
        _target = read.destination;
        _error = null;
      } else if (read == null) {
        _target = null;
      }
    });
  }

  /// Gives a PC a name. Two addresses on one dual-boot machine are unreadable otherwise, which is
  /// the report this came from.
  Future<void> _rename(PcRow row) async {
    final field = TextEditingController(text: row.known ? row.label : '');
    final name = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        backgroundColor: widget.palette.surface,
        title: Text(
          'Call this PC',
          style: TextStyle(
              fontFamily: mono, color: widget.palette.ink, fontSize: 18),
        ),
        content: TextField(
          controller: field,
          autofocus: true,
          maxLength: 24,
          style: TextStyle(fontFamily: mono, color: widget.palette.ink),
          decoration: InputDecoration(
            hintText: row.destination.host,
            counterText: '',
            hintStyle: TextStyle(
                fontFamily: mono, color: widget.palette.inkDim),
          ),
          onSubmitted: (value) => Navigator.pop(context, value),
        ),
        actions: [
          // Forgetting has to be reachable from the same place as naming. A list that can only
          // grow collects every machine that ever answered once — a friend's PC, a test, a
          // receiver somebody ran for five minutes — and none of them can be got rid of.
          if (row.known)
            TextButton(
              onPressed: () => Navigator.pop(context, _forgetSentinel),
              child: Text(
                'Forget',
                style: TextStyle(color: widget.palette.noAnswer.base),
              ),
            ),
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, field.text),
            child: const Text('Save'),
          ),
        ],
      ),
    );
    field.dispose();
    if (name == null || !mounted) return;

    if (name == _forgetSentinel) {
      setState(() {
        _known = _known.forget(row.destination);
        // The chosen PC cannot be one that is no longer on the list.
        if (_target == row.destination) {
          _target = null;
          _entry.clear();
          _entryMessage = null;
        }
      });
      _saveKnown();
      return;
    }
    // A name can be given to a PC that has only ever answered a sweep, so it is remembered here
    // rather than only when a session confirms.
    setState(() {
      _known = _known
          .remember(row.destination, now: DateTime.now().millisecondsSinceEpoch)
          .rename(row.destination, name);
    });
    _saveKnown();
  }

  Future<void> _saveKnown() async {
    try {
      await _control.invokeMethod('setPrefs', {'known': _known.encode()});
    } on PlatformException {
      // A list that could not be written back is not worth failing a session over.
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
          _pcCanClean = false;
          _pcCleaning = false;
          _pcEverBuffered = false;
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
          // Read from the PC's own replies, never inferred from the switch.
          _pcCanClean = (event['pcCanClean'] as bool?) ?? false;
          _pcCleaning = (event['pcCleaning'] as bool?) ?? false;
          if (_pcBufferedMs > 0) _pcEverBuffered = true;
          // A PC earns its place on the remembered list by ANSWERING — its own reply, not the
          // fact that somebody typed an address. A list full of machines that were never there
          // is worse than no list.
          final here = _target;
          if (_answeredMsAgo >= 0 && here != null && _known.find(here) == null) {
            _known = _known.remember(
              here,
              now: DateTime.now().millisecondsSinceEpoch,
            );
            _saveKnown();
          }
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
          // The next session starts collapsed again. Asking for detail was about *this* session.
          _expanded = false;
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
      setState(() => _error = _rows.isEmpty
          ? 'Pick your PC, or type its address. Nothing on this network has '
              'answered yet — check Earshot is running over there.'
          : 'Pick your PC from the list.');
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

    final chosen = MicSource.usable(_source, _rate);
    final args = {
      'host': target.host,
      'port': target.port,
      // The service only ever deals in an Android source; the key is this screen's business, and
      // is what gets remembered.
      'source': chosen.id,
      'rate': _rate,
      // The wish the PC will be asked for, on every packet, for the whole session -- and
      // changeable while it runs without restarting anything.
      'clean': _clean,
      'key': chosen.key,
      'known': _known.encode(),
    };
    await _control.invokeMethod('setPrefs', args);

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
      final before = _net.localIp;
      setState(() => _net = NetworkState.fromMap(net));
      // Landing on a different network means the PCs found on the last one are not reachable any
      // more, and the ones here have never been looked for. Opening the app before the Wi-Fi has
      // connected is the ordinary way this happens.
      if (!_running && _net.localIp != null && _net.localIp != before) _search();
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

  /// Turns the PC's cleaning on or off.
  ///
  /// Unlike the microphone and the rate, this works **while a session runs**: the wish travels on
  /// every packet, so there is nothing to restart and nothing to acknowledge. The switch moves at
  /// once because it is a wish; whether it took effect is reported separately by the PC, and that
  /// is what the panel draws underneath.
  Future<void> _setClean(bool value) async {
    setState(() => _clean = value);
    if (_running) {
      await _control.invokeMethod('setClean', {'clean': value});
    }
    await _control.invokeMethod('setPrefs', {'clean': value});
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
                          Icons.check_circle_outline,
                          size: 17,
                          color: theme.colorScheme.primary,
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
    final kbps =
        _packets > 0 ? (_bytes * 8 / 1000 / (_packets * 0.02)).round() : 0;

    // The confirmed session gets the screen that is almost nothing. Confirmed is the operative
    // word: this is the PC's own reply, not "we pressed Start" — see collapsed_view.dart for why
    // that distinction is the whole safety of it.
    final quiet = _link == LinkState.connected && !_expanded;

    final active = MicSource.usable(_source, _rate);
    final Widget screen = quiet
        ? Scaffold(
            key: ValueKey(_muted ? 'quiet-muted' : 'quiet-live'),
            backgroundColor: widget.palette.backdrop,
            body: CollapsedView(
              palette: widget.palette,
              state: _muted ? 'muted' : 'connected',
              level: _level,
              pcBufferedMs: _pcBufferedMs,
              answering: _answeredMsAgo >= 0,
              elapsed: _startedAt == null
                  ? Duration.zero
                  : DateTime.now().difference(_startedAt!),
              onExpand: () => setState(() => _expanded = true),
              onToggleMute: _toggleMute,
            ),
          )
        : SetupPanel(
            key: const ValueKey('panel'),
            palette: widget.palette,
            link: _link,
            running: _running,
            muted: _muted,
            reach: _reach,
            onOpenWifi: () => _control.invokeMethod('openWifiSettings'),
            rows: _rows,
            target: _target,
            searching: _searching,
            onSearch: _search,
            onPick: _pick,
            onRename: _rename,
            manualOpen: _manualOpen,
            onToggleManual: () => setState(() => _manualOpen = !_manualOpen),
            entry: _entry,
            entryMessage: _entryMessage,
            onEntryChanged: _onEntryChanged,
            micName: active.name,
            micBlurb: MicSource.byKey(_source).blurb,
            source: _source,
            rate: _rate,
            audioOpen: _audioOpen,
            onToggleAudio: () => setState(() => _audioOpen = !_audioOpen),
            networkOpen: _networkOpen,
            onToggleNetwork: () => setState(() => _networkOpen = !_networkOpen),
            onSource: (key) => setState(() => _source = key),
            onRate: (rate) => setState(() {
        _rate = rate;
        _source = MicSource.usable(_source, rate).key;
            }),
            // Both sources are Android's own and both work at either rate, so nothing is ever greyed
            // out here now. Kept because the panel asks, and a lie would be shorter.
            sourceEnabled: (key) => true,
            clean: _clean,
            // ⚠️ The PC decides whether cleaning is happening, not this switch. Cleaning at 48 kHz only
            // is the receiver's rule, so at 16 kHz the PC answers "not cleaning" and the panel says so
            // rather than this screen second-guessing it.
            pcCanClean: _pcCanClean,
            pcCleaning: _pcCleaning,
            onClean: _setClean,
            sources: [
        for (final s in MicSource.all) (key: s.key, name: s.name),
            ],
            onAbout: () => _showMicInfo(context),
            level: _level,
            actualRate: _actualRate,
            kbps: kbps,
            packets: _packets,
            answeredMsAgo: _answeredMsAgo,
            pcBufferedMs: _pcBufferedMs,
            // Greyed rather than allowed-and-then-refused: the network row directly above says why, in
            // full, and a button that can be pressed into a session that cannot work is what got us here.
            canStart: _target != null && !_reach.blocks,
            error: _error,
            onOpenSettings: _permissionBlocked
          ? () => _control.invokeMethod('openAppSettings')
          : null,
            onToggle: _toggle,
            onMute: _toggleMute,
            // The way back to the quiet screen. Only offered when there is one to go back to — a
            // confirmed session — because collapsing anything else would hide a fault.
            onCollapse: _link == LinkState.connected && _expanded
          ? () => setState(() => _expanded = false)
          : null,
            version: 'earshot $appVersion',
          );

    // 🔴 One switcher across BOTH screens, not one inside the quiet branch.
    //
    // It used to animate only between the quiet screen's own two states, because the panel was
    // returned from a different branch entirely — so hiding and coming back was a hard cut, and he
    // said it felt instant. A hard cut between two full-screen layouts reads as a crash rather
    // than a change of mind.
    //
    // 340 ms: long enough to read as a transition, short enough that nobody waits for it.
    return AnimatedSwitcher(
      duration: const Duration(milliseconds: 340),
      switchInCurve: Curves.easeOutCubic,
      switchOutCurve: Curves.easeInCubic,
      transitionBuilder: (child, animation) => FadeTransition(
        opacity: animation,
        child: ScaleTransition(
          // The arriving screen settles the last 4% inwards. Going quiet it reads as the app
          // stepping back; coming out of it, as the detail stepping forward.
          scale: Tween(begin: 1.04, end: 1.0).animate(animation),
          child: child,
        ),
      ),
      child: screen,
    );
  }
}
