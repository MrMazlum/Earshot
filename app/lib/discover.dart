// Finding the PC, so nobody has to type where it is.
//
// There is no pairing code any more and no address to read off a screen. The phone knows its own
// address and subnet, so it knocks on every door on that subnet and lists whoever answers.
//
// ## Why knocking works, and needs nothing new on the PC
//
// The receiver already replies. Since the "connected means connected" release it answers any valid
// packet from whoever is sending with a `HELLO` — a header and nothing else — so that the phone can
// tell "my audio is arriving" from "my audio is going nowhere". A keepalive is a valid packet that
// carries no audio, so one keepalive per address is a question the receiver answers by existing.
//
// That is the whole mechanism. No multicast, no mDNS, no broadcast — all three are blocked, rate
// limited or silently dropped on enough home routers and phones to be unreliable, and a plain
// unicast knock is answered by the code that already shipped.
//
// ## What it costs
//
// 254 datagrams of 16 bytes each: four kilobytes, once, when the user looks at the screen. A reply
// is 16 bytes. The sweep is over in well under a second on a quiet network.
//
// ## What it cannot do
//
// Only the phone's own subnet, and only a /24 or smaller of it. A PC behind a VPN, on another
// subnet, or on a /16 where it could be any of 65 000 addresses has to be typed in. That is what
// the address field is still for.

import 'dart:async';
import 'dart:io';
import 'dart:math';

/// Where audio is sent.
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

/// The port the receiver listens on unless told otherwise.
const int defaultPort = 47811;

const int _headerLen = 16;
const int _version = 1;
const int _typeKeepalive = 2;
const int _typeHello = 4;

/// A keepalive: a valid packet carrying no audio, which the receiver answers and ignores.
///
/// Sending audio to find a PC would be rude in a way that matters — every address on the subnet
/// would receive a fragment of whatever the microphone could hear.
List<int> probePacket(int ssrc) {
  final out = List<int>.filled(_headerLen, 0);
  out[0] = 0x45; // 'E'
  out[1] = 0x53; // 'S'
  out[2] = (_version << 4) | _typeKeepalive;
  out[3] = 0; // flags
  // sequence and timestamp stay zero: nothing is playing this.
  out[12] = (ssrc >> 24) & 0xFF;
  out[13] = (ssrc >> 16) & 0xFF;
  out[14] = (ssrc >> 8) & 0xFF;
  out[15] = ssrc & 0xFF;
  return out;
}

/// True if `data` is a reply to *our* probe.
///
/// The ssrc check is the whole of it: anyone on the network can send 16 bytes to this socket, and a
/// reply that does not carry our own session number is not evidence about us. Same reasoning as the
/// reply check in the capture service.
bool isReplyFor(int ssrc, List<int> data) {
  if (data.length < _headerLen) return false;
  if (data[0] != 0x45 || data[1] != 0x53) return false;
  if ((data[2] >> 4) != _version) return false;
  if ((data[2] & 0x0F) != _typeHello) return false;
  final theirs = (data[12] << 24) | (data[13] << 16) | (data[14] << 8) | data[15];
  return theirs == ssrc;
}

/// The addresses to knock on, given the phone's own address and subnet prefix.
///
/// Returns an empty list when a sweep makes no sense: no address, a prefix wider than a /24 (too
/// many doors), or the phone's own address being the only one there is.
///
/// The phone's own address is left out — it is not running a receiver, and knocking on it would be
/// answered by nothing while looking like a timeout.
List<String> sweepTargets(String? ip, int prefix) {
  if (ip == null) return const [];
  final parts = ip.split('.');
  if (parts.length != 4) return const [];
  final octets = parts.map(int.tryParse).toList();
  if (octets.any((o) => o == null || o < 0 || o > 255)) return const [];
  // A /24 is 254 hosts, which is the most worth knocking on. Narrower subnets (/25 and up) are
  // swept as the /24 they sit inside: a few wasted datagrams, and no arithmetic to get wrong.
  if (prefix < 24) return const [];

  final self = octets[3]!;
  final base = '${octets[0]}.${octets[1]}.${octets[2]}';
  final targets = <String>[];
  for (var host = 1; host <= 254; host++) {
    if (host == self) continue;
    targets.add('$base.$host');
  }
  return targets;
}

/// One receiver that answered, and how quickly.
class Found {
  final Destination destination;
  final Duration answeredIn;
  const Found(this.destination, this.answeredIn);
}

/// Knocks on every address on the phone's subnet and reports whoever answers.
///
/// Results arrive as a stream so the list fills in as they come, fastest first — the PC on the
/// other side of the desk usually answers in a couple of milliseconds and there is no reason to
/// make anyone wait out the timeout to see it.
///
/// The stream closes after [timeout] with however many answered.
Stream<Found> findReceivers({
  required String? selfIp,
  required int prefix,
  int port = defaultPort,
  Duration timeout = const Duration(seconds: 2),
}) {
  final targets = sweepTargets(selfIp, prefix);
  final out = StreamController<Found>();
  if (targets.isEmpty) {
    out.close();
    return out.stream;
  }

  final ssrc = Random().nextInt(0x7FFFFFFF);
  final probe = probePacket(ssrc);
  final started = Stopwatch()..start();
  final seen = <String>{};

  RawDatagramSocket.bind(InternetAddress.anyIPv4, 0).then((socket) {
    Timer? deadline;

    void finish() {
      deadline?.cancel();
      socket.close();
      if (!out.isClosed) out.close();
    }

    socket.listen(
      (event) {
        if (event != RawSocketEvent.read) return;
        final packet = socket.receive();
        if (packet == null) return;
        if (!isReplyFor(ssrc, packet.data)) return;
        final host = packet.address.address;
        // One entry per PC. A receiver answers once per probe, but a phone that sweeps twice in
        // quick succession should not list the same machine twice.
        if (!seen.add(host)) return;
        if (!out.isClosed) {
          out.add(Found(Destination(host, port), started.elapsed));
        }
      },
      onError: (_) => finish(),
      onDone: finish,
      cancelOnError: false,
    );

    for (final target in targets) {
      try {
        socket.send(probe, InternetAddress(target), port);
      } on SocketException {
        // An address the phone cannot reach at all. Nothing to do about it, and nothing to say:
        // the sweep exists precisely because we do not know which addresses are real.
      } on ArgumentError {
        // Not a usable address. Same.
      }
    }

    deadline = Timer(timeout, finish);
    out.onCancel = finish;
  }).catchError((_) {
    // No socket, no sweep. The address field is still there, which is why this is not an error
    // anyone needs to see.
    if (!out.isClosed) out.close();
  });

  return out.stream;
}
