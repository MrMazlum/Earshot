// The screen Earshot is set up on — a readout, not a settings page.
//
// Chosen by the owner on 2026-10-26 out of three finished designs, after two earlier attempts were
// turned down ("still rusty old", then "even shittier"). The three are in the history at
// `lib/setup/`; this is the one that was picked and it is the only one kept.
//
// What makes it this design and not the two it replaces:
//
//   * Monospaced, so every number lines up in a column and a glance down the left edge is the
//     whole state of the link.
//   * No boxes. One hairline closes each row, and that is the entire structure — the cards and
//     their shouting uppercase labels were the specific thing he disliked.
//   * Room to print a reason. A readout can say why it cannot start where a single large control
//     can only go grey.
//
// It is also the screen shown on top of a confirmed session when somebody asks for the detail
// back, so it carries the live numbers as a row of its own.

import 'package:flutter/material.dart';

import 'discover.dart';
import 'link_state.dart';
import 'known_pcs.dart';
import 'reachability.dart';
import 'theme.dart';

/// The app's monospace. Registered in pubspec.yaml; the whole design rests on it.
const String mono = 'JetBrainsMono';

class SetupPanel extends StatelessWidget {
  final Palette palette;

  final LinkState link;
  final bool running;
  final bool muted;

  /// What the phone can work out about reaching the PC before a packet is sent.
  final Reachability reach;
  final VoidCallback onOpenWifi;

  /// Everything answering now, then everything remembered that is not.
  final List<PcRow> rows;
  final Destination? target;
  final bool searching;
  final VoidCallback onSearch;
  final void Function(Destination) onPick;

  /// Long-press on a row. Naming is what makes two addresses on one dual-boot machine tellable
  /// apart, which is the whole reason the remembered list exists.
  final void Function(PcRow) onRename;

  /// The one box: an address, an address with a port, or a nine-digit code.
  final bool manualOpen;
  final VoidCallback onToggleManual;
  final TextEditingController entry;
  final String? entryMessage;
  final VoidCallback onEntryChanged;

  final String micName;
  final String micBlurb;
  final int source;
  final int rate;
  final bool audioOpen;
  final VoidCallback onToggleAudio;

  /// Whether the network row is unfolded. It folds like the others, because on a good day it says
  /// the same thing every time and does not need the height.
  final bool networkOpen;
  final VoidCallback onToggleNetwork;
  final void Function(int key) onSource;
  final void Function(int rate) onRate;
  final bool Function(int key) sourceEnabled;
  final List<({int key, String name})> sources;
  final VoidCallback onAbout;

  /// Whether the PC is being asked to clean the voice — a wish, set by the switch.
  final bool clean;

  /// Whether the PC says it *could* clean, and whether it says it *is*.
  ///
  /// ⚠️ Drawn instead of [clean], never alongside it as if they agreed. A receiver too old to
  /// clean answers no to both while the switch still says yes, and a panel that drew the switch
  /// would be claiming something nobody is doing — which is the mistake the green ring made.
  final bool pcCanClean;
  final bool pcCleaning;
  final void Function(bool value) onClean;

  final double level;
  final int actualRate;
  final int kbps;
  final int packets;
  final int answeredMsAgo;
  final double pcBufferedMs;

  final bool canStart;
  final String? error;
  final VoidCallback? onOpenSettings;
  final VoidCallback onToggle;
  final VoidCallback onMute;

  /// The way back to the quiet screen. Only offered when there is one to go back to.
  final VoidCallback? onCollapse;

  /// What the app calls itself at the foot. His instruction, 2026-10-26: the port and the codec
  /// came off that line — they are facts about the inside of the program, not about his PC.
  final String version;

  const SetupPanel({
    super.key,
    required this.palette,
    required this.link,
    required this.running,
    required this.muted,
    required this.reach,
    required this.onOpenWifi,
    required this.rows,
    required this.target,
    required this.searching,
    required this.onSearch,
    required this.onPick,
    required this.onRename,
    required this.manualOpen,
    required this.onToggleManual,
    required this.entry,
    required this.entryMessage,
    required this.onEntryChanged,
    required this.micName,
    required this.micBlurb,
    required this.source,
    required this.rate,
    required this.audioOpen,
    required this.onToggleAudio,
    required this.networkOpen,
    required this.onToggleNetwork,
    required this.onSource,
    required this.onRate,
    required this.sourceEnabled,
    required this.sources,
    required this.onAbout,
    required this.clean,
    required this.pcCanClean,
    required this.pcCleaning,
    required this.onClean,
    required this.level,
    required this.actualRate,
    required this.kbps,
    required this.packets,
    required this.answeredMsAgo,
    required this.pcBufferedMs,
    required this.canStart,
    required this.error,
    required this.onToggle,
    required this.onMute,
    required this.version,
    this.onOpenSettings,
    this.onCollapse,
  });

  // ---- the word in the corner -----------------------------------------------------------------

  ({String word, Color colour}) get _state {
    if (running) {
      if (muted) return (word: 'muted', colour: palette.muted.base);
      return switch (link) {
        LinkState.connected => (word: 'live', colour: palette.connected.base),
        LinkState.notPlaying => (word: 'not playing', colour: palette.noAnswer.base),
        LinkState.connecting => (word: 'waiting', colour: palette.connecting.base),
        _ => (word: 'no reply', colour: palette.noAnswer.base),
      };
    }
    if (reach.blocks) return (word: 'blocked', colour: palette.noAnswer.base);
    // A PC is chosen and the network is sound: that is ready, whether or not a sweep happens to
    // still be running. The sweep has its own flag on the row it belongs to.
    if (target != null) return (word: 'ready', colour: palette.connected.base);
    if (searching) return (word: 'scanning', colour: palette.connecting.base);
    return (word: 'choose', colour: palette.connecting.base);
  }

  @override
  Widget build(BuildContext context) {
    final p = palette;
    return Scaffold(
      backgroundColor: p.backdrop,
      body: SafeArea(
        bottom: false,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _head(p),
            Expanded(
              child: ListView(
                padding: EdgeInsets.zero,
                children: [
                  if (running) _signal(p),
                  _network(p),
                  _pcs(p),
                  _microphone(p),
                ],
              ),
            ),
            _foot(p),
            _bar(p),
          ],
        ),
      ),
    );
  }

  Widget _head(Palette p) {
    final s = _state;
    return Container(
      padding: const EdgeInsets.fromLTRB(20, 20, 20, 16),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: p.line)),
      ),
      child: Row(
        children: [
          Text(
            'EARSHOT',
            style: TextStyle(
              fontFamily: mono,
              color: p.ink,
              fontSize: 15,
              fontWeight: FontWeight.w700,
              letterSpacing: 4,
            ),
          ),
          const Spacer(),
          if (onCollapse != null)
            _Tap(
              onTap: onCollapse,
              child: Padding(
                padding: const EdgeInsets.fromLTRB(10, 6, 14, 6),
                child: Text(
                  'hide',
                  style: TextStyle(
                      fontFamily: mono, color: p.inkDim, fontSize: 13),
                ),
              ),
            ),
          Container(width: 8, height: 8, color: s.colour),
          const SizedBox(width: 9),
          Text(
            s.word,
            style: TextStyle(
              fontFamily: mono,
              color: s.colour,
              fontSize: 13,
              letterSpacing: 1.2,
            ),
          ),
        ],
      ),
    );
  }

  // ---- the rows ---------------------------------------------------------------------------------

  /// Only while streaming: what is actually going out, in the numbers that matter.
  Widget _signal(Palette p) {
    final s = _state;
    return _Row(
      palette: p,
      label: 'signal',
      flag: muted ? 'gate shut' : '${(level * 100).round()}%',
      flagColour: muted ? palette.muted.base : p.inkDim,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _Meter(palette: p, level: muted ? 0 : level, colour: s.colour),
          const SizedBox(height: 11),
          Text(
            '${(actualRate / 1000).round()} khz  $kbps kbps  $packets pkt'
            '${answeredMsAgo >= 0 ? '  pc holds ${pcBufferedMs.round()} ms' : ''}',
            style:
                TextStyle(fontFamily: mono, color: p.inkDim, fontSize: 12.5),
          ),
          // 🔴 The one state this app got wrong for its whole life: a PC that answers every packet
          // and plays none of them. It is said here in full, because "not playing" in the corner
          // tells nobody what to do about it.
          if (link == LinkState.notPlaying) ...[
            const SizedBox(height: 11),
            Text(
              'your pc is answering but playing nothing.\n'
              'it is holding 0 ms of audio, which a working one never does.\n'
              'almost always an old receiver: update it on the pc.',
              style: TextStyle(
                fontFamily: mono,
                color: p.noAnswer.base,
                fontSize: 12.5,
                height: 1.5,
              ),
            ),
          ],
        ],
      ),
    );
  }

  Widget _network(Palette p) {
    final bad = reach.blocks;
    final colour = bad ? p.noAnswer.base : p.connected.base;
    // A row that folds away a problem is a row that hides it, so trouble forces it open and takes
    // its fold away. Folding is for the day when it says "on wi-fi, same network as your pc" for
    // the hundredth time.
    final folds = !bad;
    final open = bad || networkOpen;
    return _Row(
      palette: p,
      label: 'network',
      flag: bad ? 'no route' : 'ok',
      flagColour: bad ? colour : p.inkDim,
      open: folds ? open : null,
      onToggle: folds ? onToggleNetwork : null,
      child: !open
          ? _Tap(
              onTap: onToggleNetwork,
              child: _Value(palette: p, text: reach.headline.toLowerCase()),
            )
          : Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _Value(palette: p, text: reach.headline.toLowerCase()),
          if (reach.detail != null) ...[
            const SizedBox(height: 9),
            Text(
              reach.detail!.toLowerCase(),
              style: TextStyle(
                fontFamily: mono,
                color: bad ? colour : p.inkDim,
                fontSize: 12.5,
                height: 1.5,
              ),
            ),
          ],
          if (bad) ...[
            const SizedBox(height: 12),
            _Ghost(
              palette: p,
              colour: colour,
              label: 'open wi-fi settings',
              onTap: onOpenWifi,
            ),
          ],
        ],
      ),
    );
  }

  Widget _pcs(Palette p) {
    final chosen = target;
    final live = rows.where((r) => r.live).length;
    return _Row(
      palette: p,
      label: 'your pc',
      flag: searching
          ? 'scanning'
          : live > 0
              ? '$live found'
              : rows.isEmpty
                  ? 'none'
                  : 'remembered',
      flagColour: searching ? p.connecting.base : p.inkDim,
      onLabelTap: running || searching ? null : onSearch,
      labelAction: running ? null : 'look again',
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // The list IS the value when there is a list: a big line above it would be the same
          // address printed twice, which is what the row below already says with its [x].
          if (rows.isEmpty)
            _Value(
              palette: p,
              text: chosen == null ? '— not chosen —' : _addressOf(chosen),
              dim: chosen == null,
            )
          else if (chosen == null)
            _Value(palette: p, text: '— not chosen —', dim: true),
          const SizedBox(height: 4),
          for (final row in rows)
            _PcLine(
              palette: p,
              row: row,
              chosen: chosen == row.destination,
              enabled: !running,
              onTap: () => onPick(row.destination),
              onLongPress: () => onRename(row),
            ),
          if (searching && rows.isEmpty) ...[
            const SizedBox(height: 10),
            _Sweep(palette: p, colour: p.connecting.base),
            const SizedBox(height: 9),
            Text(
              'knocking on every address on this network',
              style:
                  TextStyle(fontFamily: mono, color: p.inkDim, fontSize: 12.5),
            ),
          ],
          if (!searching && rows.isEmpty) ...[
            const SizedBox(height: 8),
            Text(
              'nothing answered. check earshot is running on the pc.',
              style: TextStyle(
                  fontFamily: mono, color: p.inkDim, fontSize: 12.5, height: 1.5),
            ),
          ],
          const SizedBox(height: 12),
          _Tap(
            onTap: running ? null : onToggleManual,
            child: Text(
              manualOpen ? '- type it in' : '+ not listed? type it in',
              style: TextStyle(
                fontFamily: mono,
                color: running ? p.line : p.inkDim,
                fontSize: 13,
              ),
            ),
          ),
          if (manualOpen) ...[
            const SizedBox(height: 12),
            _Field(
              palette: p,
              controller: entry,
              enabled: !running,
              onChanged: onEntryChanged,
              hint: '192.168.1.20',
            ),
            const SizedBox(height: 8),
            Text(
              entryMessage ??
                  'the address the pc prints when it starts. add :port only if '
                      'that pc was told to listen somewhere unusual.',
              style: TextStyle(
                fontFamily: mono,
                color: entryMessage == null ? p.inkDim : p.noAnswer.base,
                fontSize: 12,
                height: 1.5,
              ),
            ),
          ],
        ],
      ),
    );
  }

  Widget _microphone(Palette p) => _Row(
        palette: p,
        label: 'microphone',
        flag: '${rate ~/ 1000} khz',
        flagColour: p.inkDim,
        onLabelTap: onAbout,
        labelAction: 'about',
        open: audioOpen,
        // Openable while a session runs, which it did not used to be. The microphone and the rate
        // inside it are still fixed for the life of a stream -- but the cleaning switch is not,
        // and a row locked shut made the one live control in the app unreachable exactly when it
        // was wanted. Found by looking at the rendered screen, 2026-10-26.
        onToggle: onToggleAudio,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Shut, the row is one line saying what the microphone is doing. Open, the choices
            // are the line — repeating the chosen one above them would be the same words twice.
            if (!audioOpen)
              _Tap(
                onTap: onToggleAudio,
                child: _Value(palette: p, text: micName.toLowerCase()),
              )
            else ...[
              // While a session runs, the cleaning switch is the only thing in this row that can
              // still be touched, so it goes first and the inert choices go under it. Idle, the
              // order is the other way round: picking a microphone is what the row is open for.
              //
              // 🔴 Found by looking at the rendered screen: with cleaning last, the live control
              // sat below the fold behind a scroll, and the line saying whether the PC was even
              // cleaning sat below that.
              if (running) ...[
                _cleaning(p),
                const SizedBox(height: 16),
                Text(
                  'the microphone and the rate are fixed until you stop.',
                  style: TextStyle(
                      fontFamily: mono,
                      color: p.inkDim,
                      fontSize: 12.5,
                      height: 1.5),
                ),
                const SizedBox(height: 14),
              ],
              for (final s in sources)
                _Choice(
                  palette: p,
                  text: s.name.toLowerCase(),
                  chosen: s.key == source,
                  enabled: !running && sourceEnabled(s.key),
                  onTap: () => onSource(s.key),
                ),
              const SizedBox(height: 10),
              Text(
                micBlurb.toLowerCase(),
                style: TextStyle(
                    fontFamily: mono,
                    color: p.inkDim,
                    fontSize: 12.5,
                    height: 1.5),
              ),
              const SizedBox(height: 14),
              // Wrap, not Row: a rate line that is one pixel too wide for a narrow phone must
              // fall onto a second line rather than be clipped.
              Wrap(
                crossAxisAlignment: WrapCrossAlignment.center,
                spacing: 18,
                runSpacing: 8,
                children: [
                  Text(
                    'rate',
                    style: TextStyle(
                        fontFamily: mono,
                        color: p.inkDim,
                        fontSize: 12.5,
                        letterSpacing: 1.1),
                  ),
                  for (final r in const [48000, 16000])
                    _Tap(
                      onTap: running ? null : () => onRate(r),
                      child: Text(
                        '${r == rate ? '[x]' : '[ ]'}  ${r ~/ 1000} khz',
                        style: TextStyle(
                          fontFamily: mono,
                          color: r == rate ? p.ink : p.inkDim,
                          fontSize: 14,
                          fontWeight:
                              r == rate ? FontWeight.w700 : FontWeight.w400,
                        ),
                      ),
                    ),
                ],
              ),
              if (!running) ...[
                const SizedBox(height: 16),
                _cleaning(p),
              ],
            ],
          ],
        ),
      );

  /// The one control that works while the session runs, and the only noise suppression there is.
  ///
  /// The phone used to clean the voice itself. That was removed on 2026-10-26 because a keyboard
  /// came through it; the PC has the processor for a model that does not let one through, so the
  /// phone asks and the PC does it.
  ///
  /// Two lines, and they are two on purpose: the **switch** is what was asked for, and the line
  /// under it is what the PC says is happening. They disagree whenever the PC is too old, is
  /// running with cleaning switched off locally, or has a stream at a rate it cannot clean — and
  /// in every one of those cases the second line is the true one.
  Widget _cleaning(Palette p) {
    final said = !running
        ? 'the pc does it, not the phone'
        : !pcCanClean
            ? 'this pc cannot clean — an older receiver'
            : pcCleaning
                ? 'your pc is cleaning'
                : clean
                    ? 'asked for, not happening — the pc has it switched off'
                    : 'off';
    // Amber whenever cleaning was asked for and is not happening, whatever the reason -- an older
    // receiver, or one with cleaning switched off at its end. Both are the same fault from here:
    // the switch says yes and the voice is not being cleaned, and that is precisely the kind of
    // thing this app has claimed falsely before.
    final mismatch = running && clean && !pcCleaning;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _Tap(
          // Unlike the microphone and the rate, this is live: the wish travels on every packet,
          // so there is nothing to restart.
          onTap: () => onClean(!clean),
          child: Text(
            '${clean ? '[x]' : '[ ]'}  clean the noise',
            style: TextStyle(
              fontFamily: mono,
              color: clean ? p.ink : p.inkDim,
              fontSize: 14,
              fontWeight: clean ? FontWeight.w700 : FontWeight.w400,
            ),
          ),
        ),
        const SizedBox(height: 7),
        Text(
          said,
          style: TextStyle(
            fontFamily: mono,
            color: mismatch ? p.noAnswer.base : p.inkDim,
            fontSize: 12.5,
            height: 1.5,
          ),
        ),
      ],
    );
  }

  Widget _foot(Palette p) => Container(
        decoration: BoxDecoration(
          border: Border(top: BorderSide(color: p.line)),
        ),
        padding: const EdgeInsets.fromLTRB(20, 14, 20, 0),
        child: Text(
          version,
          style: TextStyle(
            fontFamily: mono,
            color: p.inkDim.withValues(alpha: 0.65),
            fontSize: 11.5,
          ),
        ),
      );

  // ---- the controls -----------------------------------------------------------------------------

  Widget _bar(Palette p) {
    final on = running || canStart;
    final stop = running;
    return SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 10, 20, 14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (error != null) ...[
              Text(
                '// ${error!.toLowerCase()}',
                style: TextStyle(
                  fontFamily: mono,
                  color: p.noAnswer.base,
                  fontSize: 12.5,
                  height: 1.5,
                ),
              ),
              if (onOpenSettings != null) ...[
                const SizedBox(height: 10),
                _Ghost(
                  palette: p,
                  colour: p.noAnswer.base,
                  label: 'open app settings',
                  onTap: onOpenSettings!,
                ),
              ],
              const SizedBox(height: 10),
            ] else if (!on) ...[
              Text(
                reach.blocks
                    ? '// cannot start: this network has no route to your pc'
                    : '// cannot start: no pc chosen',
                style: TextStyle(
                  fontFamily: mono,
                  color: reach.blocks ? p.noAnswer.base : p.inkDim,
                  fontSize: 12.5,
                ),
              ),
              const SizedBox(height: 10),
            ],
            Row(
              children: [
                if (running) ...[
                  Expanded(
                    flex: 2,
                    child: _Button(
                      palette: p,
                      label: muted ? 'UNMUTE' : 'MUTE',
                      colour: muted ? p.muted.base : null,
                      onTap: onMute,
                    ),
                  ),
                  const SizedBox(width: 12),
                ],
                Expanded(
                  flex: 3,
                  child: _Button(
                    palette: p,
                    label: stop ? 'STOP' : 'START STREAMING',
                    colour: stop ? null : (on ? p.connected.base : null),
                    onTap: on ? onToggle : null,
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  String _addressOf(Destination d) =>
      d.port == defaultPort ? d.host : '${d.host}:${d.port}';

}

// ---- the pieces the readout is made of -----------------------------------------------------------

/// One line of the readout: a label, a flag on the right, and whatever that row has to say.
/// A hairline closes it, and that is the only grouping the design uses.
class _Row extends StatelessWidget {
  final Palette palette;
  final String label;
  final String flag;
  final Color flagColour;
  final Widget child;

  /// A second, quiet control beside the label — "look again", "about".
  final String? labelAction;
  final VoidCallback? onLabelTap;

  /// A fold, drawn after the flag as [+] or [-]. Null when the row does not fold.
  final bool? open;
  final VoidCallback? onToggle;

  const _Row({
    required this.palette,
    required this.label,
    required this.flag,
    required this.flagColour,
    required this.child,
    this.labelAction,
    this.onLabelTap,
    this.open,
    this.onToggle,
  });

  @override
  Widget build(BuildContext context) {
    final p = palette;
    return Container(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 18),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: p.line)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text(
                label,
                style: TextStyle(
                  fontFamily: mono,
                  color: p.inkDim,
                  fontSize: 12.5,
                  letterSpacing: 1.1,
                ),
              ),
              if (labelAction != null) ...[
                const SizedBox(width: 14),
                _Tap(
                  onTap: onLabelTap,
                  child: Text(
                    labelAction!,
                    style: TextStyle(
                      fontFamily: mono,
                      color: onLabelTap == null ? p.line : p.inkDim,
                      fontSize: 12.5,
                      decoration: TextDecoration.underline,
                      decorationColor: p.line,
                    ),
                  ),
                ),
              ],
              const Spacer(),
              Text(
                flag,
                style: TextStyle(
                  fontFamily: mono,
                  color: flagColour,
                  fontSize: 12.5,
                  letterSpacing: 0.6,
                ),
              ),
              if (open != null) ...[
                const SizedBox(width: 14),
                _Tap(
                  onTap: onToggle,
                  child: Text(
                    open! ? '[-]' : '[+]',
                    style: TextStyle(
                      fontFamily: mono,
                      color: onToggle == null ? p.line : p.inkDim,
                      fontSize: 14,
                    ),
                  ),
                ),
              ],
            ],
          ),
          const SizedBox(height: 7),
          child,
        ],
      ),
    );
  }
}

/// The one big line in a row — the thing the row is actually about.
class _Value extends StatelessWidget {
  final Palette palette;
  final String text;
  final bool dim;
  const _Value({required this.palette, required this.text, this.dim = false});

  @override
  Widget build(BuildContext context) => Text(
        text,
        style: TextStyle(
          fontFamily: mono,
          color: dim ? palette.inkDim : palette.ink,
          fontSize: 19,
          fontWeight: FontWeight.w500,
          letterSpacing: -0.4,
        ),
      );
}

/// A PC on the list: answering now, or remembered, or both.
///
/// The marker is a shape and not only a colour, because the chosen one has to be obvious in
/// sunlight and to anybody who cannot tell the two greens apart.
class _PcLine extends StatelessWidget {
  final Palette palette;
  final PcRow row;
  final bool chosen;
  final bool enabled;
  final VoidCallback onTap;
  final VoidCallback onLongPress;

  const _PcLine({
    required this.palette,
    required this.row,
    required this.chosen,
    required this.enabled,
    required this.onTap,
    required this.onLongPress,
  });

  @override
  Widget build(BuildContext context) {
    final p = palette;
    final ink = chosen
        ? p.ink
        : row.live
            ? p.inkDim
            : p.inkDim.withValues(alpha: 0.7);
    return _Tap(
      onTap: enabled ? onTap : null,
      onLongPress: enabled ? onLongPress : null,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 7),
        child: Row(
          children: [
            Text(
              chosen ? '[x]' : '[ ]',
              style: TextStyle(
                fontFamily: mono,
                color: chosen ? p.connected.base : p.inkDim,
                fontSize: 15,
                fontWeight: FontWeight.w700,
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    row.label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontFamily: mono,
                      color: ink,
                      fontSize: 15,
                      fontWeight: chosen ? FontWeight.w700 : FontWeight.w400,
                    ),
                  ),
                  // A named PC still has to show where it is: the address is the thing that
                  // changed when he rebooted into the other operating system.
                  if (row.label != row.destination.host) ...[
                    const SizedBox(height: 2),
                    Text(
                      row.destination.port == defaultPort
                          ? row.destination.host
                          : '${row.destination.host}:${row.destination.port}',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontFamily: mono,
                        color: p.inkDim.withValues(alpha: 0.8),
                        fontSize: 12.5,
                      ),
                    ),
                  ],
                ],
              ),
            ),
            const SizedBox(width: 10),
            Text(
              row.live ? '${row.replied!.inMilliseconds} ms' : 'saved',
              style: TextStyle(
                fontFamily: mono,
                color: row.live ? p.inkDim : p.inkDim.withValues(alpha: 0.6),
                fontSize: 13,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// One of the three ways to hear.
class _Choice extends StatelessWidget {
  final Palette palette;
  final String text;
  final bool chosen;
  final bool enabled;
  final VoidCallback onTap;

  const _Choice({
    required this.palette,
    required this.text,
    required this.chosen,
    required this.enabled,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final p = palette;
    return _Tap(
      onTap: enabled ? onTap : null,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 7),
        child: Row(
          children: [
            Text(
              chosen ? '[x]' : '[ ]',
              style: TextStyle(
                fontFamily: mono,
                color: !enabled && !chosen
                    ? p.line
                    : chosen
                        ? p.connected.base
                        : p.inkDim,
                fontSize: 15,
                fontWeight: FontWeight.w700,
              ),
            ),
            const SizedBox(width: 12),
            Text(
              text,
              style: TextStyle(
                fontFamily: mono,
                color: !enabled && !chosen
                    ? p.inkDim.withValues(alpha: 0.5)
                    : chosen
                        ? p.ink
                        : p.inkDim,
                fontSize: 15,
                fontWeight: chosen ? FontWeight.w700 : FontWeight.w400,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// The one box. Monospaced, because what goes in it is an address.
class _Field extends StatelessWidget {
  final Palette palette;
  final TextEditingController controller;
  final bool enabled;
  final VoidCallback onChanged;
  final String hint;

  const _Field({
    required this.palette,
    required this.controller,
    required this.enabled,
    required this.onChanged,
    required this.hint,
  });

  @override
  Widget build(BuildContext context) {
    final p = palette;
    return TextField(
      controller: controller,
      enabled: enabled,
      onChanged: (_) => onChanged(),
      keyboardType: TextInputType.text,
      autocorrect: false,
      enableSuggestions: false,
      style: TextStyle(fontFamily: mono, color: p.ink, fontSize: 17),
      decoration: InputDecoration(
        isDense: true,
        hintText: hint,
        hintStyle:
            TextStyle(fontFamily: mono, color: p.line, fontSize: 17),
        contentPadding:
            const EdgeInsets.symmetric(horizontal: 12, vertical: 13),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(3),
          borderSide: BorderSide(color: p.line),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(3),
          borderSide: BorderSide(color: p.line),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(3),
          borderSide: BorderSide(color: p.connected.base),
        ),
      ),
    );
  }
}

/// A primary or secondary control. Hard-edged, because this design has no round things in it.
class _Button extends StatelessWidget {
  final Palette palette;
  final String label;
  final Color? colour;
  final VoidCallback? onTap;

  const _Button({
    required this.palette,
    required this.label,
    required this.colour,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final p = palette;
    final filled = colour != null;
    return Material(
      color: filled ? colour : Colors.transparent,
      borderRadius: BorderRadius.circular(4),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(4),
        child: Container(
          height: 56,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(4),
            border:
                filled ? null : Border.all(color: p.line, width: 1.5),
          ),
          child: Text(
            label,
            style: TextStyle(
              fontFamily: mono,
              color: filled
                  ? p.connected.ink
                  : onTap == null
                      ? p.inkDim
                      : p.ink,
              fontSize: 15,
              fontWeight: FontWeight.w700,
              letterSpacing: 2.4,
            ),
          ),
        ),
      ),
    );
  }
}

/// A quiet way out of a bad state. Never styled like the primary action, because it is not one.
class _Ghost extends StatelessWidget {
  final Palette palette;
  final Color colour;
  final String label;
  final VoidCallback onTap;

  const _Ghost({
    required this.palette,
    required this.colour,
    required this.label,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) => _Tap(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
          decoration: BoxDecoration(
            border: Border.all(color: colour),
            borderRadius: BorderRadius.circular(3),
          ),
          child: Text(
            label,
            style: TextStyle(
              fontFamily: mono,
              color: colour,
              fontSize: 13,
              fontWeight: FontWeight.w500,
            ),
          ),
        ),
      );
}

/// The sweep: a short lit run along a long dim line.
class _Sweep extends StatefulWidget {
  final Palette palette;
  final Color colour;
  const _Sweep({required this.palette, required this.colour});

  @override
  State<_Sweep> createState() => _SweepState();
}

class _SweepState extends State<_Sweep>
    with SingleTickerProviderStateMixin {
  late final AnimationController _c = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1100),
  )..repeat();

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => SizedBox(
        height: 2,
        child: AnimatedBuilder(
          animation: _c,
          builder: (context, _) => CustomPaint(
            painter: _SweepPainter(
              line: widget.palette.line,
              colour: widget.colour,
              t: _c.value,
            ),
            size: Size.infinite,
          ),
        ),
      );
}

class _SweepPainter extends CustomPainter {
  final Color line;
  final Color colour;
  final double t;
  _SweepPainter({required this.line, required this.colour, required this.t});

  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawRect(Offset.zero & size, Paint()..color = line);
    const run = 0.32;
    final start = (t * (1 + run)) - run;
    final left = (start.clamp(0.0, 1.0)) * size.width;
    final right = ((start + run).clamp(0.0, 1.0)) * size.width;
    if (right > left) {
      canvas.drawRect(
        Rect.fromLTRB(left, 0, right, size.height),
        Paint()..color = colour,
      );
    }
  }

  @override
  bool shouldRepaint(_SweepPainter old) => old.t != t;
}

/// The level, as blocks. A meter in a readout is made of the same characters as everything else.
class _Meter extends StatelessWidget {
  final Palette palette;
  final double level;
  final Color colour;
  const _Meter(
      {required this.palette, required this.level, required this.colour});

  static const int _cells = 28;

  @override
  Widget build(BuildContext context) {
    final lit = (level.clamp(0.0, 1.0) * _cells).round();
    return Row(
      children: [
        for (var i = 0; i < _cells; i++)
          Expanded(
            child: Container(
              margin: const EdgeInsets.only(right: 2),
              height: 18,
              color: i < lit ? colour : palette.line,
            ),
          ),
      ],
    );
  }
}

/// A tap target that keeps the readout's flatness: no ripple bleeding over a hairline, but a real
/// 48 dp area around anything a thumb has to hit.
class _Tap extends StatelessWidget {
  final Widget child;
  final VoidCallback? onTap;
  final VoidCallback? onLongPress;
  const _Tap({required this.child, this.onTap, this.onLongPress});

  @override
  Widget build(BuildContext context) => InkWell(
        onTap: onTap,
        onLongPress: onLongPress,
        borderRadius: BorderRadius.circular(3),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: child,
        ),
      );
}
