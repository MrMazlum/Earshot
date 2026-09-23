// Whether this phone could reach that PC at all — decided before anything is sent.
//
// The failure this file exists for: the phone's Wi-Fi was off, its mobile data was on, and the app
// streamed into nothing for an entire session while looking perfectly healthy. Nothing was broken.
// A pairing code is an *address*, and resolving one proves the digits were typed correctly and
// absolutely nothing else; a UDP send to an address with no route succeeds exactly like one that
// arrives.
//
// So there are two answers, and this is the first: what the phone can work out on its own, before
// pressing Start. The second is the PC's reply (Protocol.TYPE_HELLO), which is the only proof that
// packets are landing — see main.dart. This half is the one that can explain *why* they are not.
//
// Deliberately pure, and deliberately not part of any widget: it is the app's one piece of network
// reasoning, and test/reachability_test.dart is where it is held to it.

import 'pairing.dart';

/// How the phone is attached to the world. These strings come from NetworkWatch.kt; keep them in
/// step with it.
class NetworkState {
  /// `wifi`, `cellular`, `ethernet`, `vpn`, `none`, `other`, or `unknown` before the first report.
  final String transport;

  /// The phone's own IPv4 address on that network, when it has one.
  final String? ip;

  /// The subnet prefix that came with [ip] — 24 for a typical home Wi-Fi.
  final int prefix;

  const NetworkState({
    required this.transport,
    this.ip,
    this.prefix = 0,
  });

  static const unknown = NetworkState(transport: 'unknown');

  factory NetworkState.fromMap(Map<dynamic, dynamic> map) => NetworkState(
        transport: (map['transport'] as String?) ?? 'unknown',
        ip: map['ip'] as String?,
        prefix: (map['prefix'] as num?)?.toInt() ?? 0,
      );

  @override
  bool operator ==(Object other) =>
      other is NetworkState &&
      other.transport == transport &&
      other.ip == ip &&
      other.prefix == prefix;

  @override
  int get hashCode => Object.hash(transport, ip, prefix);
}

/// How bad the news is.
enum Reach {
  /// Nothing to say yet — no target typed, or the network has not reported in.
  unknown,

  /// This cannot work as things stand. Start is refused, with the reason and a way to fix it.
  blocked,

  /// It might work. Said out loud, never in the way.
  warn,

  /// Phone and PC are on the same network.
  ok,
}

/// The verdict, in the words the screen shows.
///
/// The text lives here rather than in the widget because the wording *is* the feature: "you are on
/// mobile data" is the whole answer to a session someone would otherwise spend rechecking a pairing
/// code that was right all along.
class Reachability {
  final Reach verdict;

  /// One line, always present.
  final String headline;

  /// The explanation, when there is one worth making room for.
  final String? detail;

  /// True when the Wi-Fi picker is the thing that would fix it.
  final bool offerWifi;

  const Reachability(
    this.verdict,
    this.headline, {
    this.detail,
    this.offerWifi = false,
  });

  bool get blocks => verdict == Reach.blocked;
}

/// The address blocks a home network is built from. A PC address inside one of these is on
/// somebody's LAN; an address outside them is the internet, and none of this reasoning applies.
bool isPrivateAddress(String host) {
  final octets = _ipv4(host);
  if (octets == null) return false;
  if (octets[0] == 10) return true;
  if (octets[0] == 192 && octets[1] == 168) return true;
  if (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31) return true;
  return false;
}

/// True when both addresses sit behind the same prefix — the phone's own test for "we are on the
/// same network", and the only one available without asking the router anything.
///
/// A prefix outside 1..32 means the platform did not tell us, so this refuses rather than guessing:
/// claiming two machines are on different networks when we do not know is the more expensive lie.
bool sameSubnet(String? phoneIp, int prefix, String targetHost) {
  if (phoneIp == null || prefix < 1 || prefix > 32) return false;
  final a = _ipv4(phoneIp);
  final b = _ipv4(targetHost);
  if (a == null || b == null) return false;

  final mask = prefix == 32 ? 0xFFFFFFFF : ~((1 << (32 - prefix)) - 1) & 0xFFFFFFFF;
  return (_asInt(a) & mask) == (_asInt(b) & mask);
}

/// `192.168.1.x`, for saying which network the phone is on without reciting its whole address.
String subnetLabel(String ip, int prefix) {
  final octets = _ipv4(ip);
  if (octets == null) return ip;
  if (prefix >= 24) return '${octets[0]}.${octets[1]}.${octets[2]}.x';
  if (prefix >= 16) return '${octets[0]}.${octets[1]}.x.x';
  return '${octets[0]}.x.x.x';
}

/// What to tell someone about their chances, before they press Start.
Reachability reachability(NetworkState net, Destination? target) {
  if (net.transport == 'unknown') {
    return const Reachability(Reach.unknown, '');
  }

  // No PC named yet. Say where the phone is and stop there — there is nothing to be right or
  // wrong about until there is an address to compare it to.
  if (target == null) {
    switch (net.transport) {
      case 'none':
        return const Reachability(
          Reach.unknown,
          'This phone is not on any network',
          offerWifi: true,
        );
      case 'cellular':
        return const Reachability(
          Reach.unknown,
          'This phone is on mobile data',
          detail: 'Earshot needs the phone and the PC on the same Wi-Fi.',
          offerWifi: true,
        );
      case 'vpn':
        return const Reachability(Reach.unknown, 'A VPN is on');
      default:
        return Reachability(
          Reach.unknown,
          net.transport == 'ethernet' ? 'On a wired network' : 'On Wi-Fi',
          detail: net.ip == null ? null : 'This phone is ${net.ip}.',
        );
    }
  }

  final host = target.host;
  final private = isPrivateAddress(host);

  switch (net.transport) {
    case 'none':
      return const Reachability(
        Reach.blocked,
        'This phone is not on any network',
        detail: 'Join the Wi-Fi your PC is on.',
        offerWifi: true,
      );

    case 'cellular':
      // The bug, named. Mobile data has no route to a home address, so every packet is dropped by
      // the phone itself — silently, because that is what UDP does.
      if (private) {
        return Reachability(
          Reach.blocked,
          'This phone is on mobile data',
          detail: '$host is an address on a home network, and mobile data cannot reach it. '
              'Turn Wi-Fi on and join the same network as your PC.',
          offerWifi: true,
        );
      }
      return const Reachability(
        Reach.warn,
        'This phone is on mobile data',
        detail: 'That address is not a home-network one, so this may work — but Earshot is '
            'built for a phone and a PC on the same Wi-Fi.',
      );

    case 'vpn':
      return const Reachability(
        Reach.warn,
        'A VPN is on',
        detail: 'A VPN takes your traffic off the local network, and your PC may be unreachable '
            'while it is up. If nothing arrives, turn it off and try again.',
      );

    default:
      // Wi-Fi, Ethernet, or something this phone calls neither.
      if (!private) {
        return const Reachability(Reach.ok, 'Connected to a network');
      }
      if (net.ip == null) {
        return const Reachability(
          Reach.warn,
          'No address on this network yet',
          detail: 'The phone has joined but has not been given an address. Give it a moment.',
        );
      }
      if (sameSubnet(net.ip, net.prefix, host)) {
        return Reachability(
          Reach.ok,
          net.transport == 'ethernet'
              ? 'Same network as your PC'
              : 'On Wi-Fi, same network as your PC',
          detail: 'This phone is ${net.ip}.',
        );
      }
      // Both on a network, neither on the same one: a guest Wi-Fi, a second router, or a code
      // copied from a different PC. Worth saying precisely, because the digits look right.
      return Reachability(
        Reach.warn,
        'A different network from your PC',
        detail: 'This phone is on ${subnetLabel(net.ip!, net.prefix)} and your PC is $host. '
            'They usually cannot reach each other — a guest Wi-Fi does this.',
        offerWifi: true,
      );
  }
}

List<int>? _ipv4(String text) {
  final parts = text.split('.');
  if (parts.length != 4) return null;
  final octets = <int>[];
  for (final part in parts) {
    final value = int.tryParse(part);
    if (value == null || value < 0 || value > 255) return null;
    octets.add(value);
  }
  return octets;
}

int _asInt(List<int> o) => (o[0] << 24) | (o[1] << 16) | (o[2] << 8) | o[3];
