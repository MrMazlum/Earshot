// The verdict the app reaches before it sends anything.
//
// The case at the top of this file is the one that happened: a phone on mobile data, a pairing
// code that resolved perfectly, and a whole session streamed into nothing. Everything else here
// exists so that fixing it did not quietly turn into an app that refuses to start on networks
// that work fine.

import 'package:flutter_test/flutter_test.dart';
import 'package:earshot/pairing.dart';
import 'package:earshot/reachability.dart';

const _pc = Destination('192.168.1.20', defaultPort);

NetworkState _wifi({String ip = '192.168.1.9', int prefix = 24}) =>
    NetworkState(transport: 'wifi', ip: ip, prefix: prefix);

void main() {
  group('the session that streamed into nothing', () {
    test('mobile data cannot reach a home address, and says so', () {
      final r = reachability(const NetworkState(transport: 'cellular'), _pc);

      expect(r.verdict, Reach.blocked);
      expect(r.blocks, isTrue);
      expect(r.headline, contains('mobile data'));
      // The address is named. "Check your network" would have been true and useless.
      expect(r.detail, contains('192.168.1.20'));
      expect(r.offerWifi, isTrue);
    });

    test('a phone on the same Wi-Fi is simply cleared to start', () {
      final r = reachability(_wifi(), _pc);

      expect(r.verdict, Reach.ok);
      expect(r.blocks, isFalse);
      expect(r.offerWifi, isFalse);
    });
  });

  group('what blocks, and what only warns', () {
    test('no network at all blocks', () {
      final r = reachability(const NetworkState(transport: 'none'), _pc);
      expect(r.verdict, Reach.blocked);
      expect(r.offerWifi, isTrue);
    });

    test('a different subnet warns but never blocks — it can be a working setup', () {
      final r = reachability(_wifi(ip: '192.168.5.9'), _pc);

      expect(r.verdict, Reach.warn);
      expect(r.blocks, isFalse);
      expect(r.detail, contains('192.168.5.x'));
      expect(r.detail, contains('192.168.1.20'));
    });

    test('a VPN warns rather than blocking: local traffic often still works', () {
      final r = reachability(
        const NetworkState(transport: 'vpn', ip: '10.2.0.3', prefix: 32),
        _pc,
      );
      expect(r.verdict, Reach.warn);
      expect(r.blocks, isFalse);
    });

    test('mobile data towards a public address only warns — it is not the same mistake', () {
      final r = reachability(
        const NetworkState(transport: 'cellular'),
        const Destination('203.0.113.7', defaultPort),
      );
      expect(r.verdict, Reach.warn);
      expect(r.blocks, isFalse);
    });

    test('joined but not yet given an address warns, and does not accuse the network', () {
      final r = reachability(const NetworkState(transport: 'wifi'), _pc);
      expect(r.verdict, Reach.warn);
      expect(r.blocks, isFalse);
    });
  });

  group('saying nothing when nothing is known', () {
    test('no report from Android yet is silence, not a warning', () {
      expect(reachability(NetworkState.unknown, _pc).verdict, Reach.unknown);
      expect(reachability(NetworkState.unknown, _pc).headline, isEmpty);
    });

    test('no PC typed yet: where the phone is, and no verdict about it', () {
      final r = reachability(_wifi(), null);
      expect(r.verdict, Reach.unknown);
      expect(r.blocks, isFalse);
      expect(r.headline, contains('Wi-Fi'));
    });

    test('but mobile data is worth saying even before a code is typed', () {
      final r = reachability(const NetworkState(transport: 'cellular'), null);
      expect(r.headline, contains('mobile data'));
      expect(r.blocks, isFalse); // nothing to block yet
    });
  });

  group('the subnet arithmetic', () {
    test('a /24 is the usual home network', () {
      expect(sameSubnet('192.168.1.9', 24, '192.168.1.20'), isTrue);
      expect(sameSubnet('192.168.1.9', 24, '192.168.2.20'), isFalse);
    });

    test('a /16 puts two 192.168 networks together, and a /24 does not', () {
      expect(sameSubnet('192.168.1.9', 16, '192.168.2.20'), isTrue);
      expect(sameSubnet('192.168.1.9', 24, '192.168.2.20'), isFalse);
    });

    /// A /8 network is where "same subnet" stops being a useful guess, so this checks the mask is
    /// really applied rather than the first octet being compared.
    test('a /8 is honoured exactly', () {
      expect(sameSubnet('10.1.2.3', 8, '10.250.250.250'), isTrue);
      expect(sameSubnet('10.1.2.3', 8, '11.1.2.3'), isFalse);
    });

    test('a prefix Android did not give us is refused, not guessed at', () {
      expect(sameSubnet('192.168.1.9', 0, '192.168.1.20'), isFalse);
      expect(sameSubnet('192.168.1.9', 33, '192.168.1.20'), isFalse);
      expect(sameSubnet(null, 24, '192.168.1.20'), isFalse);
    });

    test('a hostname is not an address and cannot be compared', () {
      expect(sameSubnet('192.168.1.9', 24, 'my-pc.local'), isFalse);
      expect(isPrivateAddress('my-pc.local'), isFalse);
    });
  });

  group('which addresses are home ones', () {
    test('the three private blocks, and their edges', () {
      expect(isPrivateAddress('10.0.0.1'), isTrue);
      expect(isPrivateAddress('192.168.0.1'), isTrue);
      expect(isPrivateAddress('172.16.0.1'), isTrue);
      expect(isPrivateAddress('172.31.255.255'), isTrue);

      // 172.15 and 172.32 are outside the block — the edges are where this goes wrong.
      expect(isPrivateAddress('172.15.0.1'), isFalse);
      expect(isPrivateAddress('172.32.0.1'), isFalse);
      expect(isPrivateAddress('11.0.0.1'), isFalse);
      expect(isPrivateAddress('8.8.8.8'), isFalse);
    });

    test('junk is not an address', () {
      expect(isPrivateAddress('192.168.1'), isFalse);
      expect(isPrivateAddress('192.168.1.256'), isFalse);
      expect(isPrivateAddress(''), isFalse);
    });
  });

  test('the label is the network, not the phone', () {
    expect(subnetLabel('192.168.5.9', 24), '192.168.5.x');
    expect(subnetLabel('192.168.5.9', 16), '192.168.x.x');
    expect(subnetLabel('10.5.9.2', 8), '10.x.x.x');
  });
}
