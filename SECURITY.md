# Security

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Use GitHub's private reporting instead — the **Security** tab on this repository, then *Report a
vulnerability*. It opens a private thread visible only to the maintainer.

Useful things to include: what an attacker has to be able to do first (be on the same Wi-Fi? send
one datagram? already have the pairing code?), what they get, and a way to reproduce it. A packet
that crashes the receiver is best sent as a hex dump or a short script that builds it — please do
not attach a capture of your own network.

This is a one-person hobby project. Expect a reply within a week, and expect a fix to arrive as a
new tagged release rather than a backported patch. There is no bounty.

## What Earshot promises

- **Nothing leaves your local network.** No analytics, no crash reporting, no remote config, no
  update check, no phone home. The receiver's only outbound socket call is a `connect()` on a UDP
  socket used to ask the kernel which interface has the default route — `connect()` on UDP sends no
  packet, and it is only reached when no ordinary LAN address could be found
- **Nothing is downloaded or executed on your behalf.** On Windows, when no virtual audio cable is
  installed, Earshot opens the vendor's own download page in your browser and waits. It never
  fetches the driver and never elevates an installer. See
  [`receiver/src/cable.rs`](receiver/src/cable.rs)
- **No account, no server, no cloud.** There is nothing to breach because there is nothing running
  anywhere but your own two devices

## What Earshot does *not* promise

These are known and deliberate for the current version. They are listed here so nobody has to
discover them the hard way, and none of them is a vulnerability report:

- **The audio is not encrypted.** It crosses your Wi-Fi as plain PCM. Anyone who can capture
  traffic on your network can listen to it. Encryption is planned and is not implemented
- **The receiver does not authenticate the sender.** Anyone on your LAN who knows the port can send
  audio to a running receiver and have it played, or fed into your virtual microphone. The receiver
  binds `0.0.0.0` by default; `--listen` narrows it to one interface
- **The pairing code is not a secret and not a password.** It is a reversible encoding of a private
  IP address and a port, the algorithm is public, and it protects nothing. It exists so there is one
  field to type instead of two and so your subnet stays out of your screenshots. If a report
  describes recovering an address from a code, that is the documented design
- **The Android APK in releases is signed with the Android debug key.** It is fine for sideloading
  and is not a basis for trusting the build. Proper signing is not done yet

## Scope

In scope: anything that lets a party on your LAN crash the receiver, read memory it should not,
execute code, or reach outside your network. Malformed and hostile datagrams are explicitly in
scope — the parser is written on the assumption that every packet is an attack.

Out of scope: the four items above, and anything that requires the attacker to already be running
code on one of your two machines.

## Supported versions

The latest tagged release only.
