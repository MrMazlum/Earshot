// The PCs this phone has used before, so nobody types one twice.
//
// His report, 2026-10-26, about somebody else using Earshot: a dual-boot machine is two machines
// as far as a network is concerned. Windows and Linux on the same box get different addresses, so
// the thing he had typed in yesterday is wrong today and he retypes the whole address every time
// he reboots.
//
// So the phone keeps the ones that worked. A PC earns its place by *answering* — the receiver's
// own reply, not the fact that somebody typed an address — and it keeps that place until eight
// newer ones have pushed it off the end. It can be given a name, which is the part that makes the
// dual-boot case bearable: two rows called "Windows" and "Linux" instead of two addresses nobody
// can tell apart.
//
// Stored as JSON in the same SharedPreferences the rest of the settings live in.

import 'dart:convert';

import 'discover.dart';

/// A PC that has answered this phone before.
class KnownPc {
  final String host;
  final int port;

  /// What the owner called it, or null while it is still just an address.
  final String? name;

  /// When it last answered, as milliseconds since the epoch. Only used for the order.
  final int lastSeen;

  const KnownPc({
    required this.host,
    required this.port,
    this.name,
    required this.lastSeen,
  });

  Destination get destination => Destination(host, port);

  /// What to show on the row: the name if it has one, the address otherwise.
  String get label => (name == null || name!.isEmpty) ? host : name!;

  KnownPc copyWith({String? name, int? lastSeen}) => KnownPc(
        host: host,
        port: port,
        name: name ?? this.name,
        lastSeen: lastSeen ?? this.lastSeen,
      );

  Map<String, Object?> toJson() => {
        'host': host,
        'port': port,
        if (name != null && name!.isNotEmpty) 'name': name,
        'seen': lastSeen,
      };

  static KnownPc? fromJson(Object? raw) {
    if (raw is! Map) return null;
    final host = raw['host'];
    final port = raw['port'];
    if (host is! String || host.isEmpty || port is! int) return null;
    final name = raw['name'];
    final seen = raw['seen'];
    return KnownPc(
      host: host,
      port: port,
      name: name is String && name.isNotEmpty ? name : null,
      lastSeen: seen is int ? seen : 0,
    );
  }
}

/// The remembered list, newest first.
///
/// Immutable: every change returns a new list, so the screen can hold one and the caller cannot
/// mutate it from under a build.
class KnownPcs {
  /// How many are kept. Eight is more machines than anybody has and still a short list to read.
  static const int limit = 8;

  final List<KnownPc> all;

  const KnownPcs(this.all);

  static const empty = KnownPcs([]);

  bool get isEmpty => all.isEmpty;

  KnownPc? find(Destination d) {
    for (final pc in all) {
      if (pc.host == d.host && pc.port == d.port) return pc;
    }
    return null;
  }

  /// What a destination is called on screen — its name if it has one, its address if not.
  String labelFor(Destination d) => find(d)?.label ?? d.host;

  /// Moves a PC to the front, keeping whatever name it already had.
  ///
  /// Called when a receiver answers, and only then. An address that was typed but never replied is
  /// not a PC this phone knows — remembering those is how a list fills up with machines that were
  /// never there.
  KnownPcs remember(Destination d, {required int now}) {
    final existing = find(d);
    final rest = all.where((pc) => pc.host != d.host || pc.port != d.port);
    return KnownPcs([
      existing?.copyWith(lastSeen: now) ??
          KnownPc(host: d.host, port: d.port, lastSeen: now),
      ...rest,
    ].take(limit).toList(growable: false));
  }

  /// Gives one a name, or clears the name when given an empty string.
  KnownPcs rename(Destination d, String name) {
    final trimmed = name.trim();
    return KnownPcs([
      for (final pc in all)
        if (pc.host == d.host && pc.port == d.port)
          KnownPc(
            host: pc.host,
            port: pc.port,
            name: trimmed.isEmpty ? null : trimmed,
            lastSeen: pc.lastSeen,
          )
        else
          pc,
    ]);
  }

  KnownPcs forget(Destination d) => KnownPcs([
        for (final pc in all)
          if (pc.host != d.host || pc.port != d.port) pc,
      ]);

  String encode() => jsonEncode([for (final pc in all) pc.toJson()]);

  /// Anything unreadable comes back empty rather than throwing: a corrupted preference must not
  /// stop the app opening, and the worst case is one list the owner has to rebuild by connecting.
  static KnownPcs decode(String? raw) {
    if (raw == null || raw.isEmpty) return empty;
    try {
      final parsed = jsonDecode(raw);
      if (parsed is! List) return empty;
      final out = <KnownPc>[];
      for (final item in parsed) {
        final pc = KnownPc.fromJson(item);
        if (pc != null) out.add(pc);
      }
      return KnownPcs(out.take(limit).toList(growable: false));
    } catch (_) {
      return empty;
    }
  }
}

/// One row of the PC list: either something that just answered, or something remembered, or both.
///
/// The merge is the whole point. A remembered PC that is also answering right now must be ONE row
/// — with its name and its reply time — and not the same machine listed twice.
class PcRow {
  final Destination destination;
  final String label;

  /// How fast it answered the sweep, or null when it is only a memory.
  final Duration? replied;

  /// True when it answered this sweep.
  bool get live => replied != null;

  /// True when the phone has used it before.
  final bool known;

  const PcRow({
    required this.destination,
    required this.label,
    required this.replied,
    required this.known,
  });
}

/// The list the screen draws: everything answering now, then everything remembered that is not.
///
/// Live machines first because one of them is almost always the answer; the remembered ones stay
/// visible underneath so a PC that is off, or on a network the sweep cannot reach, is still one
/// tap away instead of something to type again.
List<PcRow> mergePcs(List<Found> found, KnownPcs known) {
  final rows = <PcRow>[
    for (final f in found)
      PcRow(
        destination: f.destination,
        label: known.labelFor(f.destination),
        replied: f.answeredIn,
        known: known.find(f.destination) != null,
      ),
  ];
  for (final pc in known.all) {
    final already = rows.any((row) =>
        row.destination.host == pc.host && row.destination.port == pc.port);
    if (already) continue;
    rows.add(PcRow(
      destination: pc.destination,
      label: pc.label,
      replied: null,
      known: true,
    ));
  }
  return rows;
}
