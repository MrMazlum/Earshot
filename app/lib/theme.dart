// The app's looks, as three complete palettes rather than one set of colours scattered through the
// widgets.
//
// Why three: the app is being dressed once, properly, and then left alone. Choosing between three
// finished things is a decision somebody can actually make; choosing between adjectives is not.
//
// Every palette answers the same five questions, because the app has five states and they are what
// anybody actually reads off the screen:
//
//     idle · connecting · connected · no answer · muted
//
// A palette says what each of those *is*, so a screen can be built from the state alone and never
// from a hard-coded colour. That is the whole reason this file exists — the old UI had one seed
// green and asked the eye to tell five states apart by reading words.

import 'package:flutter/material.dart';

/// What a state looks like: the colour it owns, and the ink that stays legible on it.
class StateColours {
  final Color base;
  final Color ink;
  const StateColours(this.base, this.ink);
}

/// One complete look.
class Palette {
  final String name;

  /// One line on what the look is for, shown on the comparison sheet and nowhere else.
  final String idea;

  final Brightness brightness;

  /// Behind everything.
  final Color backdrop;

  /// Cards and raised surfaces.
  final Color surface;

  /// Body text.
  final Color ink;

  /// Secondary text — present, quieter, never below readable.
  final Color inkDim;

  /// Lines and edges.
  final Color line;

  /// The colour the app is "about", used for the primary action.
  final Color accent;

  final StateColours idle;
  final StateColours connecting;
  final StateColours connected;
  final StateColours noAnswer;
  final StateColours muted;

  /// How much of the confirmed screen the state's colour takes.
  ///
  /// True floods it: the whole phone becomes the state, readable across a room and impossible to
  /// misread. False keeps the dark (or pale) field and spends the colour on the ring alone, which
  /// is quieter at 2am and on a phone lying beside a bed. This is the real difference between the
  /// three looks, not a shade.
  final bool floodWhenLive;

  /// The font the palette asks for. Null means the platform's own, which is Roboto on Android and
  /// is what the screenshot harness registers.
  final String? fontFamily;

  const Palette({
    required this.name,
    required this.idea,
    required this.brightness,
    required this.backdrop,
    required this.surface,
    required this.ink,
    required this.inkDim,
    required this.line,
    required this.accent,
    required this.idle,
    required this.connecting,
    required this.connected,
    required this.noAnswer,
    required this.muted,
    required this.floodWhenLive,
    this.fontFamily,
  });

  ThemeData get theme {
    final scheme = ColorScheme.fromSeed(
      seedColor: accent,
      brightness: brightness,
    ).copyWith(
      surface: surface,
      onSurface: ink,
      primary: accent,
      onPrimary: brightness == Brightness.dark ? backdrop : Colors.white,
      outline: line,
    );
    return ThemeData(
      useMaterial3: true,
      brightness: brightness,
      colorScheme: scheme,
      fontFamily: fontFamily ?? 'Roboto',
      scaffoldBackgroundColor: backdrop,
      dividerColor: line,
      hintColor: inkDim,
      inputDecorationTheme: InputDecorationTheme(
        border: const OutlineInputBorder(),
        isDense: true,
        hintStyle: TextStyle(color: inkDim),
      ),
    );
  }

  /// The colours for one state, by the same name the screen uses.
  StateColours of(String state) => switch (state) {
        'connecting' => connecting,
        'connected' => connected,
        'noAnswer' => noAnswer,
        'muted' => muted,
        _ => idle,
      };
}

/// **Signal** — his choice, 2026-10-26, and inverted on his instruction.
///
/// Every state owns a hue, and that hue is what you *see*: the ground stays black and the colour
/// is spent on the thing that was black before — the ring, the words, the mark. His words: *"can
/// we invert the colors (black background and the color in previously black parts) in all parts
/// but setting up page"*.
///
/// What that buys beyond taste: a black field on an OLED phone lying beside a bed at 2am costs
/// almost no light and almost no battery, while a flooded screen is a lamp. The state is still
/// read as a colour from across a room, because the ring is large and the ground gives it maximum
/// contrast.
const signal = Palette(
  name: 'Signal',
  idea: 'Black ground, and every state owns a colour. The colour is the ring, not the room.',
  brightness: Brightness.dark,
  backdrop: Color(0xFF0B1310),
  surface: Color(0xFF121C18),
  ink: Color(0xFFE8F2ED),
  inkDim: Color(0xFF8AA399),
  line: Color(0xFF203029),
  accent: Color(0xFF3DDC97),
  floodWhenLive: false,
  idle: StateColours(Color(0xFF4A5B55), Color(0xFFE8F2ED)),
  connecting: StateColours(Color(0xFFE2B33C), Color(0xFF1A1405)),
  connected: StateColours(Color(0xFF3DDC97), Color(0xFF05170F)),
  noAnswer: StateColours(Color(0xFFE2573C), Color(0xFF200803)),
  muted: StateColours(Color(0xFFBE8CE8), Color(0xFF17091F)),
);

/// **Instrument** — dark, one accent, and states told apart by weight rather than hue.
///
/// A studio box, not an app: charcoal, a single amber, and the difference between states carried by
/// fill against outline and by how much of the screen the state takes. Quieter than Signal, and the
/// one that will still look right in five years.
const instrument = Palette(
  name: 'Instrument',
  idea: 'Charcoal and one amber. The colour stays in the ring; the room stays dark.',
  brightness: Brightness.dark,
  backdrop: Color(0xFF14110E),
  surface: Color(0xFF1D1A16),
  ink: Color(0xFFF1EBE2),
  inkDim: Color(0xFF9C9387),
  line: Color(0xFF2E2922),
  accent: Color(0xFFE8A33D),
  floodWhenLive: false,
  idle: StateColours(Color(0xFF3A342C), Color(0xFFF1EBE2)),
  connecting: StateColours(Color(0xFF6B5A3A), Color(0xFFF1EBE2)),
  connected: StateColours(Color(0xFFE8A33D), Color(0xFF17120A)),
  noAnswer: StateColours(Color(0xFF8C3A2E), Color(0xFFF7E9E6)),
  muted: StateColours(Color(0xFF4A423A), Color(0xFFC9BFB2)),
);

/// **Paper** — light, ink on warm white, colour spent only where it means something.
///
/// The opposite bet: a phone lying on a desk in daylight, legible at arm's length, and nothing
/// glowing. Colour appears for exactly two states — live, and broken — so when it appears it is
/// information rather than decoration.
const paper = Palette(
  name: 'Paper',
  idea: 'Light, ink on warm white. Colour appears only for live and for broken.',
  brightness: Brightness.light,
  backdrop: Color(0xFFF6F3EC),
  surface: Color(0xFFFFFDF8),
  ink: Color(0xFF1B1A17),
  inkDim: Color(0xFF6E6A61),
  line: Color(0xFFDCD6C9),
  accent: Color(0xFF1F6F4A),
  floodWhenLive: false,
  idle: StateColours(Color(0xFFDCD6C9), Color(0xFF1B1A17)),
  connecting: StateColours(Color(0xFFCBB069), Color(0xFF1B1A17)),
  connected: StateColours(Color(0xFF1F6F4A), Color(0xFFF6F3EC)),
  noAnswer: StateColours(Color(0xFFB2412C), Color(0xFFFDF1EE)),
  muted: StateColours(Color(0xFFBDB6A7), Color(0xFF2A2721)),
);

/// The three, in the order they are meant to be looked at.
const palettes = <Palette>[signal, instrument, paper];

/// The one the app ships with until somebody says otherwise.
const defaultPalette = signal;
