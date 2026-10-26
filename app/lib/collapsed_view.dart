// The screen once the PC has answered: almost nothing.
//
// His ask, 2026-10-25: *"Once we are connected and its confirmed the ui should be way more
// minimalised. We basically dont need to see anything at all."*
//
// That is right, and it is also the one place where hiding things is safe. Everything on the full
// screen — the address, the list of PCs, the mic sources, the meter, the rate — exists to get you
// *into* a working session. Once you are in one, the phone is lying face down being a microphone,
// and the only thing a hand reaches for is mute.
//
// ## Two rules this screen obeys
//
// 1. **It is earned by confirmation, never by hope.** It appears only while the PC is actually
//    answering. The moment that stops, the full screen comes back by itself — a minimal screen
//    that hides a fault would undo the entire point of the release that made the badge honest.
// 2. **Nothing here is the only way to do anything.** A tap anywhere brings the detail back, and
//    the notification still carries mute and stop, so a screen this bare can never trap anybody.
//
// ## What moves, and why
//
// One thing moves: a ring that follows what the microphone is hearing. It is not decoration — it
// is the answer to "is this thing still listening to me", which is the only question left once the
// link is confirmed. Everything else fades: after a few seconds of nobody touching the phone the
// words go quiet and the colour is the whole interface.

import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'theme.dart';

/// How long after the last touch the words fade down to almost nothing.
const _settleAfter = Duration(seconds: 6);

/// The collapsed, connected screen.
class CollapsedView extends StatefulWidget {
  final Palette palette;

  /// 'connected' or 'muted'. Any other state and this screen should not be on display at all.
  final String state;

  /// What the microphone is hearing, 0..1. Drives the only thing that moves.
  final double level;

  /// How long the session has been up, as the one number worth keeping on screen.
  final Duration elapsed;

  final VoidCallback onExpand;
  final VoidCallback onToggleMute;

  /// Set by the screenshot harness so a still frame is deterministic. Null means normal behaviour:
  /// the words settle after [_settleAfter].
  final bool? settledOverride;

  const CollapsedView({
    super.key,
    required this.palette,
    required this.state,
    required this.level,
    required this.elapsed,
    required this.onExpand,
    required this.onToggleMute,
    this.settledOverride,
  });

  @override
  State<CollapsedView> createState() => _CollapsedViewState();
}

class _CollapsedViewState extends State<CollapsedView>
    with SingleTickerProviderStateMixin {
  late final AnimationController _breath = AnimationController(
    vsync: this,
    duration: const Duration(seconds: 4),
  )..repeat(reverse: true);

  bool _settled = false;
  Timer? _settle;

  @override
  void initState() {
    super.initState();
    _touched();
  }

  @override
  void dispose() {
    _settle?.cancel();
    _breath.dispose();
    super.dispose();
  }

  /// Any touch wakes the words back up, and the clock starts again.
  ///
  /// Does nothing at all when [CollapsedView.settledOverride] is set: that means something else is
  /// deciding — the screenshot harness — and a timer left running there is a test that fails on a
  /// picture it took correctly.
  void _touched() {
    if (widget.settledOverride != null) return;
    _settle?.cancel();
    setState(() => _settled = false);
    _settle = Timer(_settleAfter, () {
      if (mounted) setState(() => _settled = true);
    });
  }

  @override
  Widget build(BuildContext context) {
    final muted = widget.state == 'muted';
    final colours = widget.palette.of(widget.state);
    final settled = widget.settledOverride ?? _settled;

    // Two ways to say the same thing, and the palette decides which. Flooding makes the phone the
    // state — unmissable from across a room, and a lot of light at 2am. Not flooding keeps the
    // room dark and spends the colour on the ring and the words, which is the same information at
    // a tenth of the brightness.
    // Inverted: a black ground, and the state's colour spent on what used to be black. The tint
    // is deliberately tiny — enough that the ground is not pure void, not enough to be a lamp.
    final field = widget.palette.floodWhenLive
        ? colours.base
        : Color.lerp(const Color(0xFF000000), colours.base, 0.06)!;
    final ink = widget.palette.floodWhenLive ? colours.ink : colours.base;

    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () {
        _touched();
        widget.onExpand();
      },
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 420),
        curve: Curves.easeOutCubic,
        color: field,
        child: SafeArea(
          child: Stack(
            children: [
              Center(
                child: AnimatedBuilder(
                  animation: _breath,
                  builder: (context, _) {
                    // Muted: a slow breath, so the screen still looks alive while the microphone
                    // deliberately is not. Live: the ring follows the voice.
                    final pulse = muted
                        ? 0.06 * math.sin(_breath.value * math.pi)
                        : widget.level.clamp(0.0, 1.0) * 0.30;
                    return _Ring(
                      ink: ink,
                      pulse: pulse,
                      hollow: muted,
                      // Colour on black needs more of itself than dark-on-colour does: the same
                      // alphas that read as rings on a flooded field read as smudges here.
                      bold: !widget.palette.floodWhenLive,
                    );
                  },
                ),
              ),
              // The words. They are the part that goes away — the colour is what remains.
              Positioned(
                left: 0,
                right: 0,
                top: 36,
                child: AnimatedOpacity(
                  duration: const Duration(milliseconds: 600),
                  opacity: settled ? 0.28 : 1,
                  child: Column(
                    children: [
                      Text(
                        muted ? 'MUTED' : 'LIVE',
                        style: TextStyle(
                          color: ink,
                          fontSize: 13,
                          letterSpacing: 4,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                      const SizedBox(height: 8),
                      Text(
                        _clock(widget.elapsed),
                        style: TextStyle(
                          color: ink.withValues(alpha: 0.75),
                          fontSize: 34,
                          fontWeight: FontWeight.w300,
                          fontFeatures: const [FontFeature.tabularFigures()],
                        ),
                      ),
                    ],
                  ),
                ),
              ),
              // The only control. Big, low, and reachable with a thumb on a phone lying on a desk.
              Positioned(
                left: 0,
                right: 0,
                bottom: 42,
                child: Center(
                  child: _MuteButton(
                    ink: ink,
                    muted: muted,
                    onTap: () {
                      _touched();
                      widget.onToggleMute();
                    },
                  ),
                ),
              ),
              Positioned(
                left: 0,
                right: 0,
                bottom: 14,
                child: AnimatedOpacity(
                  duration: const Duration(milliseconds: 600),
                  opacity: settled ? 0 : 0.55,
                  child: Text(
                    'tap anywhere for details',
                    textAlign: TextAlign.center,
                    style: TextStyle(color: ink, fontSize: 12),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

String _clock(Duration d) {
  final m = d.inMinutes.remainder(60).toString().padLeft(2, '0');
  final s = d.inSeconds.remainder(60).toString().padLeft(2, '0');
  return d.inHours > 0 ? '${d.inHours}:$m:$s' : '$m:$s';
}

/// The one thing that moves: concentric rings that swell with what the microphone hears.
class _Ring extends StatelessWidget {
  final Color ink;

  /// 0..0.3 of extra radius.
  final double pulse;

  /// Muted draws the rings open and thin — present, but plainly not carrying anything.
  final bool hollow;

  /// True when the rings are the colour and the ground is black, which needs more of the colour.
  final bool bold;

  const _Ring({
    required this.ink,
    required this.pulse,
    required this.hollow,
    required this.bold,
  });

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: 260,
      height: 260,
      child: CustomPaint(
        painter: _RingPainter(ink: ink, pulse: pulse, hollow: hollow, bold: bold),
      ),
    );
  }
}

class _RingPainter extends CustomPainter {
  final Color ink;
  final double pulse;
  final bool hollow;
  final bool bold;

  _RingPainter({
    required this.ink,
    required this.pulse,
    required this.hollow,
    required this.bold,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final centre = Offset(size.width / 2, size.height / 2);
    final base = size.width * 0.22;

    // Three rings, each a little further out and a little fainter: a sound leaving, not a loading
    // spinner. They move together so the whole thing reads as one breath.
    for (var i = 0; i < 3; i++) {
      final spread = 1 + i * 0.42;
      final radius = base * spread * (1 + pulse * (1 - i * 0.22));
      canvas.drawCircle(
        centre,
        radius,
        Paint()
          ..color = ink.withValues(
            alpha: (hollow ? (bold ? 0.42 : 0.22) : (bold ? 0.9 : 0.42)) /
                (1 + i * 0.9),
          )
          ..style = PaintingStyle.stroke
          ..strokeWidth = hollow ? 2 : (6 - i * 1.4),
      );
    }

    if (!hollow) {
      canvas.drawCircle(
        centre,
        base * 0.52 * (1 + pulse * 0.6),
        Paint()..color = ink.withValues(alpha: 0.9),
      );
    } else {
      // Muted: the centre is a line through the circle, which is the one shape nobody misreads.
      canvas.drawCircle(
        centre,
        base * 0.52,
        Paint()
          ..color = ink.withValues(alpha: 0.6)
          ..style = PaintingStyle.stroke
          ..strokeWidth = 3,
      );
      canvas.drawLine(
        centre + Offset(-base * 0.4, base * 0.4),
        centre + Offset(base * 0.4, -base * 0.4),
        Paint()
          ..color = ink.withValues(alpha: 0.85)
          ..strokeCap = StrokeCap.round
          ..strokeWidth = 4,
      );
    }
  }

  @override
  bool shouldRepaint(covariant _RingPainter old) =>
      old.pulse != pulse || old.hollow != hollow || old.ink != ink || old.bold != bold;
}

/// Mute, as a shape rather than a word: the only control on the screen should not need reading.
class _MuteButton extends StatelessWidget {
  final Color ink;
  final bool muted;
  final VoidCallback onTap;

  const _MuteButton({required this.ink, required this.muted, required this.onTap});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 250),
        width: 96,
        height: 96,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          color: muted ? ink.withValues(alpha: 0.16) : Colors.transparent,
          border: Border.all(color: ink.withValues(alpha: 0.5), width: 2),
        ),
        child: Icon(
          muted ? Icons.mic_off : Icons.mic_none,
          color: ink,
          size: 38,
        ),
      ),
    );
  }
}
