// What the sweep knocks on, and what it refuses to knock on.
//
// The case this file exists for turned up the day it was written: his phone had Tailscale running,
// so Android's *active* network was the tunnel and the app read its address — 100.102.242.x, which
// is a private range of its own and has nothing to do with his Wi-Fi. Sweeping that would have
// found nothing, every time, while looking exactly like a PC that was switched off.

import 'package:flutter_test/flutter_test.dart';

import 'package:earshot/discover.dart';
import 'package:earshot/reachability.dart';

void main() {
  group('sweepTargets', () {
    test('covers the subnet and leaves out the phone itself', () {
      final targets = sweepTargets('192.168.1.9', 24);
      expect(targets.length, 253); // 1..254, minus this phone
      expect(targets.first, '192.168.1.1');
      expect(targets.last, '192.168.1.254');
      expect(targets, isNot(contains('192.168.1.9')));
      // .0 and .255 are the network and broadcast addresses, not machines.
      expect(targets, isNot(contains('192.168.1.0')));
      expect(targets, isNot(contains('192.168.1.255')));
    });

    test('a narrower subnet is swept as the /24 it sits in', () {
      expect(sweepTargets('192.168.1.9', 25).length, 253);
    });

    test('refuses a subnet too wide to knock on door by door', () {
      // A /16 is 65 000 addresses. Knocking on all of them is not a search, it is a flood.
      expect(sweepTargets('172.16.4.9', 16), isEmpty);
      expect(sweepTargets(null, 24), isEmpty);
      expect(sweepTargets('not an address', 24), isEmpty);
    });
  });

  group('the probe and the reply', () {
    test('a probe is a keepalive, carrying no audio and our own ssrc', () {
      final probe = probePacket(0x01020304);
      expect(probe.length, 16);
      expect(probe[0], 0x45); // 'E'
      expect(probe[1], 0x53); // 'S'
      expect(probe[2], (1 << 4) | 2); // version 1, KEEPALIVE
      expect(probe.sublist(12), [0x01, 0x02, 0x03, 0x04]);
    });

    test('a reply is only ours if it carries our ssrc', () {
      final hello = <int>[
        0x45, 0x53, (1 << 4) | 4, 0, // magic, version 1, HELLO
        0, 0, 0, 0, // sequence
        0, 0, 0, 0, // timestamp
        0x01, 0x02, 0x03, 0x04, // ssrc
      ];
      expect(isReplyFor(0x01020304, hello), isTrue);
      // Someone else's session, answering someone else's phone.
      expect(isReplyFor(0x05060708, hello), isFalse);
      // Not a HELLO, not ours, not long enough.
      expect(isReplyFor(0x01020304, [...hello]..[2] = (1 << 4) | 3), isFalse);
      expect(isReplyFor(0x01020304, [...hello]..[0] = 0x44), isFalse);
      expect(isReplyFor(0x01020304, hello.sublist(0, 12)), isFalse);
    });
  });

  group('a VPN underneath', () {
    test('the search uses the Wi-Fi address, not the tunnel', () {
      final net = NetworkState.fromMap({
        'transport': 'vpn',
        'ip': '100.102.242.17', // Tailscale
        'prefix': 32,
        'localIp': '192.168.1.9',
        'localPrefix': 24,
      });
      // What the warning is about stays the tunnel; what gets knocked on is the Wi-Fi.
      expect(net.ip, '100.102.242.17');
      expect(sweepTargets(net.localIp, net.localPrefix), contains('192.168.1.13'));
      expect(sweepTargets(net.ip, net.prefix), isEmpty);
    });

    test('a PC on the Wi-Fi under the tunnel is not called unreachable', () {
      final net = NetworkState.fromMap({
        'transport': 'vpn',
        'ip': '100.102.242.17',
        'prefix': 32,
        'localIp': '192.168.1.9',
        'localPrefix': 24,
      });
      final verdict = reachability(net, const Destination('192.168.1.13', 47811));
      expect(verdict.verdict, Reach.warn);
      expect(verdict.headline, contains('your PC is on this Wi-Fi'));
      expect(verdict.blocks, isFalse);
    });

    test('an older app half sends no local address, and the active one is used', () {
      final net = NetworkState.fromMap({
        'transport': 'wifi',
        'ip': '192.168.1.9',
        'prefix': 24,
      });
      expect(net.localIp, '192.168.1.9');
      expect(net.localPrefix, 24);
    });
  });
}
