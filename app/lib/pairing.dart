// Pairing codes — the phone half. This must agree with receiver/src/pairing.rs digit for digit.
//
// That file carries the full explanation: what the code is, what it deliberately is not (it is an
// encoding, not encryption), and why it is built the way it is. Read it before changing anything
// here. Every constant below appears there too, and the two are checked against the same vectors
// in test/pairing_test.dart.
//
// The arithmetic needs 64-bit integers: the widest intermediate is about 5.5e16. Dart gives that
// on the VM and in AOT, which is every Android build. It would silently lose precision on the web,
// where ints are doubles — so if Earshot ever grows a web client, this is the file that breaks.

/// The port the receiver listens on unless told otherwise.
const int defaultPort = 47811;

const int _portSlots = 8;
const int _modulus = 999999937;
const int _multiplier = 387420489;
const int _multiplierInverse = 115270698;

const List<int> _digitOrder = [4, 7, 1, 8, 0, 6, 3, 5, 2];
const List<int> _digitShift = [3, 1, 4, 1, 5, 9, 2, 6, 5];

/// `(first address as a 32-bit number, how many, index of the first)`.
const List<List<int>> _blocks = [
  [0xC0A80000, 1 << 16, 0], // 192.168.0.0/16
  [0xAC100000, 1 << 20, 1 << 16], // 172.16.0.0/12
  [0x0A000000, 1 << 24, (1 << 16) + (1 << 20)], // 10.0.0.0/8
];

const int _addressSpace = (1 << 16) + (1 << 20) + (1 << 24);
const int _payloadSpace = _addressSpace * _portSlots;

/// Where a pairing code points.
class Destination {
  final String host;
  final int port;
  const Destination(this.host, this.port);

  @override
  bool operator ==(Object other) =>
      other is Destination && other.host == host && other.port == port;

  @override
  int get hashCode => Object.hash(host, port);

  @override
  String toString() => '$host:$port';
}

/// The address and port a code stands for, or null if it stands for nothing.
///
/// Roughly seven out of eight single-digit typos land here, which is the point: the user gets
/// "that code is not right" straight away instead of a connection that never arrives.
Destination? resolvePairingCode(String text) {
  final code = _digitsOf(text);
  if (code == null) return null;

  final scattered = _undiffuse(code);
  // _diffuse works on all nine-digit strings, so undiffusing can land above the modulus.
  if (scattered >= _modulus) return null;

  final payload = (scattered * _multiplierInverse) % _modulus;
  if (payload >= _payloadSpace) return null;

  final index = payload ~/ _portSlots;
  for (final block in _blocks.reversed) {
    if (index >= block[2]) {
      return Destination(_ipv4(block[0] + index - block[2]),
          defaultPort + payload % _portSlots);
    }
  }
  return null;
}

/// The code for an address and port, or null if it cannot be expressed.
///
/// The phone does not need this — the receiver generates the codes — but having both directions in
/// one place is what makes the test able to prove the two halves agree.
String? pairingCodeFor(String host, int port) {
  final value = _parseIpv4(host);
  if (value == null) return null;
  final slot = port - defaultPort;
  if (slot < 0 || slot >= _portSlots) return null;

  int? index;
  for (final block in _blocks) {
    if (value >= block[0] && value - block[0] < block[1]) {
      index = block[2] + (value - block[0]);
      break;
    }
  }
  if (index == null) return null;

  final payload = index * _portSlots + slot;
  return _diffuse((payload * _multiplier) % _modulus)
      .toString()
      .padLeft(9, '0');
}

/// Groups a code in threes for display: `123456789` becomes `123 456 789`.
String groupPairingCode(String code) {
  final d = code.padLeft(9, '0');
  return '${d.substring(0, 3)} ${d.substring(3, 6)} ${d.substring(6, 9)}';
}

/// True once the field holds something that could be a code — nine digits, however grouped.
/// Says nothing about whether it resolves.
bool looksLikePairingCode(String text) => _digitsOf(text) != null;

/// Nine digits with only spaces, dashes or underscores between them.
///
/// A dot is pointedly not allowed: `192.168.1.42` is nine digits too, and reading a pasted address
/// as some unrelated machine's code is the worst answer available.
int? _digitsOf(String text) {
  final trimmed = text.trim();
  final buffer = StringBuffer();
  for (final unit in trimmed.codeUnits) {
    if (unit >= 0x30 && unit <= 0x39) {
      buffer.writeCharCode(unit);
    } else if (unit != 0x20 && unit != 0x2D && unit != 0x5F) {
      return null; // not a space, dash or underscore
    }
  }
  final digits = buffer.toString();
  return digits.length == 9 ? int.tryParse(digits) : null;
}

int _diffuse(int value) {
  final digits = _nineDigits(value);
  var out = 0;
  for (var i = 0; i < 9; i++) {
    out = out * 10 + (digits[_digitOrder[i]] + _digitShift[i]) % 10;
  }
  return out;
}

int _undiffuse(int value) {
  final shuffled = _nineDigits(value);
  final digits = List<int>.filled(9, 0);
  for (var i = 0; i < 9; i++) {
    digits[_digitOrder[i]] = (shuffled[i] + 10 - _digitShift[i]) % 10;
  }
  return digits.fold(0, (acc, d) => acc * 10 + d);
}

List<int> _nineDigits(int value) {
  final digits = List<int>.filled(9, 0);
  var rest = value;
  for (var i = 8; i >= 0; i--) {
    digits[i] = rest % 10;
    rest ~/= 10;
  }
  return digits;
}

String _ipv4(int value) =>
    '${(value >> 24) & 0xFF}.${(value >> 16) & 0xFF}.${(value >> 8) & 0xFF}.${value & 0xFF}';

int? _parseIpv4(String text) {
  final parts = text.split('.');
  if (parts.length != 4) return null;
  var value = 0;
  for (final part in parts) {
    final octet = int.tryParse(part);
    if (octet == null || octet < 0 || octet > 255) return null;
    value = (value << 8) | octet;
  }
  return value;
}
