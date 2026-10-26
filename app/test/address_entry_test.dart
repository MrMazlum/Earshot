// One box, three kinds of entry, and the refusals that have to be exact.

import 'package:flutter_test/flutter_test.dart';
import 'package:earshot/address_entry.dart';
import 'package:earshot/discover.dart';
import 'package:earshot/pairing.dart';

void main() {
  test('an address on its own takes the usual port', () {
    final read = readDestination('192.168.1.20')!;
    expect(read.destination, const Destination('192.168.1.20', 47811));
  });

  test('an address with a port keeps that port', () {
    final read = readDestination(' 192.168.1.20:47900 ')!;
    expect(read.destination, const Destination('192.168.1.20', 47900));
  });

  test('a code resolves to what it stands for', () {
    final code = pairingCodeFor('192.168.1.20', 47811)!;
    final read = readDestination(groupPairingCode(code))!;
    expect(read.destination, const Destination('192.168.1.20', 47811));
  });

  test('a code that stands for nothing is refused as a code, not as an address', () {
    // Nine digits is the shape; whether it decodes is the question. Walk until one is rejected,
    // because most nine-digit strings are valid codes by construction.
    AddressReading? refused;
    for (var i = 0; i < 400 && refused == null; i++) {
      final text = (123456789 + i).toString();
      final read = readDestination(text)!;
      if (!read.isOk) refused = read;
    }
    expect(refused, isNotNull, reason: 'no nine-digit string in the range was refused');
    expect(refused!.fault, AddressFault.badCode);
  });

  test('a dotted address is never read as a code', () {
    // 192.168.1.42 is nine digits with dots between them. Reading it as some unrelated machine's
    // code would be the worst available answer.
    final read = readDestination('192.168.1.42')!;
    expect(read.destination, const Destination('192.168.1.42', 47811));
  });

  test('a Tailscale address goes through, since no code can express one', () {
    final read = readDestination('100.85.223.11')!;
    expect(read.destination, const Destination('100.85.223.11', 47811));
  });

  test('rubbish after the colon is a port fault, not an address fault', () {
    expect(readDestination('192.168.1.20:')!.fault, AddressFault.badPort);
    expect(readDestination('192.168.1.20:hello')!.fault, AddressFault.badPort);
    expect(readDestination('192.168.1.20:0')!.fault, AddressFault.badPort);
    expect(readDestination('192.168.1.20:70000')!.fault, AddressFault.badPort);
  });

  test('something that is not an address at all says so', () {
    expect(readDestination('my-pc')!.fault, AddressFault.badAddress);
    expect(readDestination('192.168.1')!.fault, AddressFault.badAddress);
    expect(readDestination('999.1.1.1')!.fault, AddressFault.badAddress);
  });

  test('an empty box is not an error', () {
    expect(readDestination(''), isNull);
    expect(readDestination('   '), isNull);
  });

  test('what comes back out is what would have been typed in', () {
    expect(writeDestination(const Destination('192.168.1.20', 47811)),
        '192.168.1.20');
    expect(writeDestination(const Destination('192.168.1.20', 47900)),
        '192.168.1.20:47900');
  });
}
