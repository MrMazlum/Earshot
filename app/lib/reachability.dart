// Whether this phone could reach that PC at all — decided before anything is sent.
//
// The failure this file exists for: the phone's Wi-Fi was off, its mobile data was on, and the app
// streamed into nothing for an entire session while looking perfectly healthy. Nothing was broken.
// Knowing where the PC is proves only that an address was found or typed, and
// absolutely nothing else; a UDP send to an address with no route succeeds exactly like one that
// arrives.
//
// So there are two answers, and this is the first: what the phone can work out on its own, before
// pressing Start. The second is the PC's reply (Protocol.TYPE_HELLO), which is the only proof that
// packets are landing — see main.dart. This half is the one that can explain *why* they are not.
//
// Deliberately pure, and deliberately not part of any widget: it is the app's one piece of network
// reasoning, and test/reachability_test.dart is where it is held to it.

import 'discover.dart';

/// How the phone is attached to the world. These strings come from NetworkWatch.kt; keep them in
/// step with it.
class NetworkState {
  /// `wifi`, `cellular`, `ethernet`, `vpn`, `none`, `other`, or `unknown` before the first report.
  final String transport;

  /// The phone's own IPv4 address on that network, when it has one.
  final String? ip;

  /// The subnet prefix that came with [ip] — 24 for a typical home Wi-Fi.
  final int prefix;

  /// The address on the *local* network, which is the same as [ip] unless a VPN is up.
  ///
  /// With Tailscale or any other tunnel running, the active network is the tunnel and [ip] is its
  /// address — a 100.64-100.127 one, a private range of its own that says nothing about this
  /// Wi-Fi. The PC can only be found on the network underneath, so that is the address the search
  /// uses. [ip] stays the tunnel's, because the warning about it is right.
  final String? localIp;

  /// The prefix that came with [localIp].
  final int localPrefix;

  const NetworkState({
    required this.transport,
    this.ip,
    this.prefix = 0,
    this.localIp,
    this.localPrefix = 0,
  });

  static const unknown = NetworkState(transport: 'unknown');

  factory NetworkState.fromMap(Map<dynamic, dynamic> map) {
    final ip = map['ip'] as String?;
    final prefix = (map['prefix'] as num?)?.toInt() ?? 0;
    return NetworkState(
      transport: (map['transport'] as String?) ?? 'unknown',
      ip: ip,
      prefix: prefix,
      // An older build of the app half sends neither; falling back to the active network is what
      // it used to do, and is right whenever no VPN is up.
      localIp: (map['localIp'] as String?) ?? ip,
      localPrefix: (map['localPrefix'] as num?)?.toInt() ?? prefix,
    );
  }

  @override
  bool operator ==(Object other) =>
      other is NetworkState &&
      other.transport == transport &&
      other.ip == ip &&
      other.prefix == prefix &&
      other.localIp == localIp &&
      other.localPrefix == localPrefix;

  @override
  int get hashCode => Object.hash(transport, ip, prefix, localIp, localPrefix);
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
/// mobile data" is the whole answer to a session someone would otherwise spend rechecking an
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

/// True for an address that is not on anybody's home network — a public one, reachable from the
/// internet.
///
/// Only decidable for a literal IPv4 address. A name (`desktop.local`, a hostname) is not judged
/// here: resolving it is the network's job and guessing would block the one case that works.
bool isPublicAddress(String host) {
  final octets = _ipv4(host);
  if (octets == null) return false;
  if (isPrivateAddress(host)) return false;
  // 100.64–100.127 is carrier-grade NAT, which is what Tailscale hands out. Private in every sense
  // that matters here: it is not reachable from the internet and it is somebody's own machines.
  if (octets[0] == 100 && octets[1] >= 64 && octets[1] <= 127) return false;
  if (octets[0] == 127) return false; // loopback, which is a developer testing
  if (octets[0] == 169 && octets[1] == 254) return false; // link-local
  return true;
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

  // Refused outright, on every network, before anything else is considered.
  //
  // Earshot sends the microphone **unencrypted** — that is written in the protocol and in the
  // README, and it is fine on a home network. A public address is a live microphone crossing the
  // open internet in the clear, past every machine on the way. Nothing about the rest of this
  // function matters if that is what is about to happen.
  if (isPublicAddress(host)) {
    return Reachability(
      Reach.blocked,
      'That address is on the internet',
      detail: '$host is not a home-network address, and Earshot sends your microphone without '
          'encryption. Use an address on your own network, or a VPN that puts your PC on it.',
    );
  }

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
      // A tunnel is the active network, but most of them — Tailscale among them — only carry
      // their own addresses and leave local traffic alone. So the question is not "is a VPN on",
      // it is "is the PC still on the Wi-Fi underneath".
      if (private && sameSubnet(net.localIp, net.localPrefix, host)) {
        // Short on purpose. This is the ordinary state for anyone running Tailscale, so it is not
        // news and does not deserve four lines explaining itself every time the screen opens.
        return Reachability(
          Reach.warn,
          'VPN on, PC on this wi-fi',
          detail: 'This phone is ${net.localIp}. Usually fine.',
        );
      }
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
