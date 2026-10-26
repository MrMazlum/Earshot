// One box for typing a PC in, whatever form it is in.
//
// His instruction, 2026-10-26 evening: *"if we cant get rid of the port then lets use our old
// adress option, so we dont have to enter an extra box."* There is no second box any more and no
// switch between two kinds of entry. One field takes all three things anybody has:
//
//     192.168.1.20          an address, on the usual port
//     192.168.1.20:47900    an address whose receiver was told to listen somewhere else
//     482 915 003           a nine-digit code, which carries the port inside it
//
// Which one it is can be worked out from the text, so nobody has to say.
//
// ⚠️ **Nothing produces a code today.** The receiver prints its address and its port, and the
// Windows tray copies the address — checked 2026-10-26. The code is still accepted, because one
// written down earlier still works and accepting it costs nothing, but it is not offered on
// screen: telling somebody to read nine digits off a PC that never shows them is worse than
// having no shortcut at all. A dotted address can
// never be read as a code — resolvePairingCode refuses anything containing a dot, exactly so that
// `192.168.1.42` is not quietly resolved to an unrelated machine.

import 'discover.dart';
import 'pairing.dart';

/// Why a typed entry was refused, in the words the screen shows.
enum AddressFault {
  /// Nine digits that decode to nothing. Most single-digit typos land here.
  badCode,

  /// The part after the colon is not a port.
  badPort,

  /// Nothing usable at all.
  badAddress,
}

/// What a typed entry turned into: a destination, or the reason it is not one.
class AddressReading {
  final Destination? destination;
  final AddressFault? fault;

  const AddressReading.ok(Destination this.destination) : fault = null;
  const AddressReading.bad(AddressFault this.fault) : destination = null;

  bool get isOk => destination != null;

  /// What to put under the field. Null while the field is still empty or still right.
  String? get message => switch (fault) {
        AddressFault.badCode => 'That code does not stand for an address.',
        AddressFault.badPort => 'After the colon there has to be a port number.',
        AddressFault.badAddress => 'An address looks like 192.168.1.20.',
        null => null,
      };
}

/// Reads whatever was typed. Null for an empty field, which is not an error — it is a field nobody
/// has filled in yet.
AddressReading? readDestination(String text) {
  final trimmed = text.trim();
  if (trimmed.isEmpty) return null;

  if (looksLikePairingCode(trimmed)) {
    final resolved = resolvePairingCode(trimmed);
    return resolved == null
        ? const AddressReading.bad(AddressFault.badCode)
        : AddressReading.ok(resolved);
  }

  final colon = trimmed.indexOf(':');
  final host = colon < 0 ? trimmed : trimmed.substring(0, colon);
  if (!_looksLikeHost(host)) {
    return const AddressReading.bad(AddressFault.badAddress);
  }

  // No colon is the ordinary case, and the ordinary case must not ask for a port: every receiver
  // listens on 47811 unless somebody went out of their way to change it.
  if (colon < 0) return AddressReading.ok(Destination(host, defaultPort));

  final port = int.tryParse(trimmed.substring(colon + 1).trim());
  if (port == null || port < 1 || port > 65535) {
    return const AddressReading.bad(AddressFault.badPort);
  }
  return AddressReading.ok(Destination(host, port));
}

/// How a destination is written back into the box.
///
/// The port is left off when it is the usual one, so what comes back out is what would have been
/// typed in rather than a line with a number on the end nobody chose.
String writeDestination(Destination d) =>
    d.port == defaultPort ? d.host : '${d.host}:${d.port}';

/// Four numbers separated by dots, each 0-255.
///
/// Deliberately not a hostname: nothing in the app resolves names, so accepting one here would be
/// a field that takes an entry the rest of the app cannot use.
bool _looksLikeHost(String text) {
  final parts = text.split('.');
  if (parts.length != 4) return false;
  for (final part in parts) {
    if (part.isEmpty || part.length > 3) return false;
    final octet = int.tryParse(part);
    if (octet == null || octet < 0 || octet > 255) return false;
  }
  return true;
}
