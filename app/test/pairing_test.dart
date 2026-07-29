// Proves the phone decodes exactly what the receiver encodes.
//
// The vectors in protocol/pairing-vectors.csv are read by receiver/src/pairing.rs too. If either
// side ever drifts, one of the two suites goes red — which is a much better outcome than the phone
// quietly dialling a machine that is not there.

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:earshot/pairing.dart';

void main() {
  test('agrees with the shared vectors, both directions', () {
    final file = File('../protocol/pairing-vectors.csv');
    expect(file.existsSync(), isTrue, reason: 'run this from the app/ directory');

    var checked = 0;
    for (final line in file.readAsLinesSync()) {
      if (line.isEmpty || line.startsWith('#')) continue;
      final f = line.split(',');
      final code = f[0], host = f[1], port = int.parse(f[2]);

      expect(pairingCodeFor(host, port), code, reason: '$host:$port');
      expect(resolvePairingCode(code), Destination(host, port), reason: code);
      // However a person groups it while typing.
      expect(resolvePairingCode(groupPairingCode(code)), Destination(host, port));
      checked++;
    }
    expect(checked, greaterThanOrEqualTo(10),
        reason: 'has the vectors file been emptied?');
  });

  test('a code that stands for nothing resolves to nothing', () {
    // Not nine digits.
    expect(resolvePairingCode('12345678'), isNull);
    expect(resolvePairingCode('1234567890'), isNull);
    expect(resolvePairingCode(''), isNull);
    // Nine digits, but an address — reading this as a code would send the phone somewhere else
    // entirely, so the dot has to be fatal.
    expect(resolvePairingCode('192.168.1.42'), isNull);
    expect(resolvePairingCode('abc123456'), isNull);
  });

  test('most single-digit typos are refused rather than dialled', () {
    const code = '335618795'; // 192.168.1.42:47811
    var tried = 0, accepted = 0;
    for (var position = 0; position < 9; position++) {
      for (var digit = 0; digit < 10; digit++) {
        final wrong = code.replaceRange(position, position + 1, '$digit');
        if (wrong == code) continue;
        tried++;
        if (resolvePairingCode(wrong) != null) accepted++;
      }
    }
    expect(accepted * 4, lessThan(tried),
        reason: '$accepted of $tried typos still decoded');
  });

  test('an address outside the private blocks has no code', () {
    for (final host in [
      '8.8.8.8', // public
      '127.0.0.1', // loopback
      '169.254.3.4', // link-local
      '172.32.0.1', // just past 172.16/12
      '100.64.0.1', // carrier-grade NAT
    ]) {
      expect(pairingCodeFor(host, defaultPort), isNull, reason: host);
    }
  });

  test('only the ports a code can carry get one', () {
    expect(pairingCodeFor('10.1.2.3', defaultPort + 7), isNotNull);
    expect(pairingCodeFor('10.1.2.3', defaultPort + 8), isNull);
    expect(pairingCodeFor('10.1.2.3', defaultPort - 1), isNull);
  });
}
