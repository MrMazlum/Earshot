# Pre-open-source checklist

This repo is **private** until every box below is ticked. The plan is to publish under GPL-3.0.
Owner's instruction (2026-07-29): *"after all of it is done we will make sure there is no security
leak and then we will make it open sourced."*

## What is left (reviewed 2026-07-30)

Everything below is ticked except four items, none of which is a code change:

1. Run a real secret scanner (`gitleaks` / `trufflehog`) — neither is installed on this machine.
   The manual `git log -p --all` pass is done and clean, so this is a second opinion, not a
   suspicion
2. Turn on GitHub secret scanning + push protection, in the repo settings, **before** flipping it
   public
3. ⚠️ Do not `git push --all` — see §1. Only `main` and the version tags belong on the remote
4. When the repo goes public, delete the "these links 404 while the repo is private" note from the
   README's Download table. Nothing else in the README needs changing

One real security finding turned up in this review and is **fixed**: a remote stall in the reorder
window, §4.

## 1. History, not just the working tree
Deleting a secret in a later commit does not remove it — it stays in the git history and in every
clone. Scan the **whole history**, and if anything is found, rewrite it before publishing (or start
the public repo from a fresh, squashed initial commit).

- [x] **History squashed to a single root commit (2026-07-29).** Everything before it is gone from
      the published history, so the only thing left to scan is what is reachable from `main`
- [x] `git log -p --all | grep -iE 'token|secret|password|api[_-]?key|BEGIN .*PRIVATE KEY|keystore'`
      — manual pass done 2026-07-30. Every hit is the *word* appearing in documentation that says
      not to commit these things (`.gitignore`, `CONTRIBUTING.md`, this file, the GPL text, and
      `GH_TOKEN: ${{ github.token }}`, which is the CI-provided token and not a value). No secret
- [ ] Scan with a real secret scanner (`gitleaks detect`, `trufflehog`) — neither is installed here
- [ ] Enable GitHub secret scanning + push protection on the repo before it goes public
- [ ] ⚠️ **Do not `git push --all`.** A local `backup-pre-squash` branch still holds the pre-squash
      history. It has been reviewed and contains no secret, but it is not meant to be published, and
      the remote currently has only `main`, `v0.1.0` and `v0.1.1`. Delete it or keep pushing by name

## 2. Credentials and signing
- [x] No Android keystore, `key.properties`, or upload key in the tree or history. `.gitignore`
      covers `*.jks`, `*.keystore`, `key.properties`; `git log -p --all` confirms none was ever
      added
- [x] No `.env`, no service-account JSON, no API tokens. `.gitignore` covers `*.pem`, `*.p12`,
      `*.env`, `.env.*`, `secrets/`
- [x] Nothing to rotate, because nothing was ever committed
- [x] **The release APK is signed with the Android debug key**, deliberately and temporarily. That
      is disclosed in `SECURITY.md` and commented in `app/android/app/build.gradle.kts`. It is a
      known limitation, not a leak — the debug key is not a secret

## 3. Personal and network data
- [x] No home IP addresses, MAC addresses, SSIDs, or router details in code, docs, logs or test
      vectors. `git grep -E '\b(192\.168|10\.[0-9]+\.|172\.(1[6-9]|2[0-9]|3[01])\.)…'` returns only
      the made-up addresses in `protocol/pairing-vectors.csv`, in doc examples, and in unit tests
- [x] No device serial numbers (ADB IDs) committed — `adb` has never seen the phone; the APK is
      installed over the network
- [x] No voice recordings in the repo — `.gitignore` covers `*.wav`, `*.flac`, `*.pcap`, `*.pcapng`
- [x] No screenshots or logs containing the LAN topology. The only committed images are the app
      icon and its generated launcher sizes
- [x] **The private project notebook is not published, and nothing in the tree points at it any
      more.** 24 source and doc comments used to cite `~/EarshotBrain/…`, a path no reader outside
      this machine has. Each was either rewritten to explain itself or repointed at
      `protocol/README.md` or `CONTRIBUTING.md`, which now carry that content for real
- [x] No absolute local paths containing the owner's username in tracked files. `CONTRIBUTING.md`
      is the version that ships
- [x] **No local tooling fingerprints in the published tree.** The patterns for editor/assistant
      config used to sit in the tracked `.gitignore`, where the pattern list itself would have been
      published. They now live in `.git/info/exclude`, which is local to the clone and never
      pushed. `git grep` for them comes back empty
- [x] Pairing-code test vectors use made-up addresses only (`protocol/pairing-vectors.csv`)

## 4. Security review of the code itself
Publishing invites people to look for holes — better to find them first.

- [x] **Packet parser**: bounds-checked and non-panicking. `Header::parse` checks the length before
      any indexing, verifies the magic, and refuses an unknown version rather than misreading it;
      `pcm_rate_from_payload` rejects an odd or out-of-range payload; an oversized datagram is
      truncated by `recv_from` into a 4096-byte buffer and then fails the rate check. There is no
      `unsafe` anywhere in the parse path, and no arithmetic that can overflow into an index. Tests:
      `rejects_junk`, `pcm_rate_is_derived_from_length`, `sequence_wrap_is_handled`
- [x] 🔴 **One real finding, fixed (2026-07-30): a remote stall in the reorder window.** `Reorder`
      had no bound on how far *ahead* a sequence number could be. `pop` declares one loss per
      missing sequence, so a few datagrams claiming a sequence ~2 billion ahead — which any device
      on the LAN can send, unsolicited — put the receive thread into a loop of two billion
      iterations, each pushing a frame of concealment silence. Not a crash and not an over-read; the
      receiver simply never produced audio again. Fixed with `MAX_AHEAD` (50 frames, one second):
      beyond that it resyncs to the new sequence instead of concealing the gap. The same bound also
      fixes an honest bug — a multi-second Wi-Fi dropout used to push seconds of silence into the
      ring and leave the call permanently behind. Regression test:
      `sequence_numbers_from_the_far_future_cannot_stall_the_loop`, verified to fail without the fix
- [x] **Pairing**: the nine-digit code *identifies* a PC, it does not authenticate one. Accepted and
      published as a limitation rather than fixed — it is documented as such in `SECURITY.md`, in
      the README, and at the top of `receiver/src/pairing.rs`. Anyone already on the LAN can send
      audio to a running receiver. Encryption and authentication are future work, and the project is
      honest about that instead of implying otherwise
- [x] **Encryption**: there is none, and the README and `SECURITY.md` both say so plainly. The
      `ENC` flag is reserved in the wire format and unused. `TYPE_PCM_DEBUG` is not compiled out
      because it is not a debug path any more — it is the only payload type implemented, and
      `protocol/README.md` documents it as what ships today
- [x] **Binding**: `0.0.0.0` is deliberate, not accidental. The phone may be on any interface, and
      binding one guessed interface would be the same class of bug as the VPN-tunnel address that
      already cost real debugging time. `--listen` narrows it for anyone who wants that. Verified by
      trace: exactly one `AF_INET` socket is ever created, and it is only ever bound
- [x] **Dependencies**: every runtime dependency is GPL-3.0-compatible — cpal `Apache-2.0`, alsa
      `Apache-2.0/MIT`, alsa-sys `MIT`, dasp_sample `MIT OR Apache-2.0`, if-addrs
      `MIT OR BSD-3-Clause`, ksni `Unlicense`, dbus / dbus-tree / libdbus-sys `Apache-2.0/MIT`,
      thiserror / libc / bitflags / cfg-if `MIT OR Apache-2.0`
- [x] `cargo audit` is not installed here, so the advisories were checked by hand. Two crates in the
      tree are unmaintained and carry RUSTSEC advisories — `ansi_term` (RUSTSEC-2021-0139) and
      `atty` (RUSTSEC-2021-0145). Both arrive via `clap 2` → `dbus-codegen`, which is a **build
      dependency** of `ksni`: they run on the build machine, are Linux-only, and are not linked into
      any shipped binary. `cargo tree -i clap` shows the path. Recheck if ksni ever moves them to a
      runtime dependency
- [x] **Privacy claim verified by trace, not by assertion.** `strace -f -e trace=network` over a
      full run: one `AF_INET` socket, bound to `0.0.0.0` and never `connect`ed or `sendto`ed; zero
      `connect()` to any internet address; no `sin_addr` in the trace other than the bind. The only
      other sockets are `AF_UNIX` (D-Bus session bus, PipeWire) and one `AF_NETLINK` `NETLINK_ROUTE`
      socket, which is `if_addrs` asking the kernel for the local interface list. **Not one byte was
      addressed off the machine.** The `connect("8.8.8.8:80")` route probe in `engine.rs` was never
      reached — it is a last resort when no LAN address is found, and `connect` on a UDP socket
      sends no packet in any case

## 5. Licence and attribution
- [x] `LICENSE` present and correct — the full GPL-3.0 text. `Cargo.toml` declares
      `license = "GPL-3.0-or-later"`. The project convention is no per-file headers
- [x] Third-party licences acknowledged: the crate list and its licences are in §4 above. libopus
      and Oboe are **not** dependencies yet — neither is linked, so neither is acknowledged as if it
      were. Add them when they land
- [x] The Windows VB-Cable dependency disclosed in the README as a closed-source third-party
      component the user installs themselves ("The honest parts")
- [x] **The guided install never fetches or executes anything.** `receiver/src/cable.rs` detects a
      missing cable, explains it, and opens VB-Audio's own page in the browser. Downloading the
      driver pack and elevating it for the user was considered and rejected: it is a third-party
      kernel driver, there is no published checksum to verify a download against, and teaching users
      to accept a silent elevated install is a bad habit to build into a tool. If that ever changes,
      it needs a checksum shown to the user and an explicit opt-in flag

## 6. Publication hygiene
- [x] README honest about status and about latency. Under-100 ms is stated as a **target** and
      explicitly flagged as unmeasured: *"That will be measured and published here, or not claimed
      at all."* The Windows row says the build has never been run by a human; the macOS row says
      "nothing — never built, not in CI"
- [x] `.github/ISSUE_TEMPLATE/bug_report.yml` — opens by asking people **not** to paste IP
      addresses, pairing codes, Wi-Fi names or packet captures, and routes security reports away
      from public issues via `config.yml`
- [x] `SECURITY.md` — private reporting through GitHub advisories, plus an explicit list of what
      Earshot does and does not promise, so the four known limitations are not filed as findings
- [x] `.github/pull_request_template.md` — makes the "state your latency cost in ms" rule something
      a contributor is actually asked for
- [x] Squash or review the pre-public commit history for anything embarrassing or identifying
- [x] No dangling pointers into anything unpublished: `git grep EarshotBrain` is clean apart from
      this file's own record of the fix
