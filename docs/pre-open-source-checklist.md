# Pre-open-source checklist

This was the gate this repository had to clear before it was published under GPL-3.0.
Owner's instruction (2026-07-29): *"after all of it is done we will make sure there is no security
leak and then we will make it open sourced."* It cleared it on **2026-07-30**.

## ✅ Published 2026-07-30

Every box below is ticked and the repository is public. This file stays in the tree on purpose:
what was actually checked is more useful to a reader — and to whoever maintains this next — than a
claim that it was.

**What was done on the day, in order:**

1. **Commit email replaced throughout the history.** All 15 commits and all 5 annotated tags carried
   the owner's personal address, which publication would have made permanent in every clone and
   fork. Rewritten to GitHub's `190613564+MrMazlum@users.noreply.github.com` **before** the repo was
   public, i.e. while no clone of it existed anywhere. Verified afterwards: the old address appears
   zero times in any commit, tag object or file; every commit still resolves to the GitHub account
   `MrMazlum`; and **every one of the 15 commits has a byte-identical tree to before the rewrite** —
   only metadata changed. `git config user.email` is pinned locally so a future commit cannot
   reintroduce it. ⚠️ That is a *repository-local* setting: a fresh clone inherits the global one
   again.
2. **`backup-pre-squash` deleted.** The final sweep found six `Co-Authored-By:` trailers naming an
   AI assistant — on that branch and only that branch, never on `main`, never on the remote. It made
   a single `git push --all` enough to break a standing rule permanently. Archived to a `git bundle`
   outside the repository, then deleted, so the mistake is now impossible rather than merely
   documented. §1.
3. **Secret scanning and push protection enabled** the moment the repo went public — they are not
   offered to private repositories on the free plan, so "before flipping" was not achievable and
   "within the same minute" was. Dependabot alerts and security updates on as well.
4. **README notes that only made sense while private removed**, and the download table verified to
   return `200` unauthenticated for all three artefacts.

**One accepted, disclosed limitation:** the Android APK is signed with the debug key. It is in
`SECURITY.md` and in §2 below, and it is a known state, not an oversight.

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
- [x] **Pattern scan over the full history, 2026-07-30.** `gitleaks`/`trufflehog` are not installed
      here, so the whole `git log -p --all` (22,707 diff lines) was scanned against the twelve
      patterns those tools lead with: GitHub PATs (`ghp_`, `github_pat_`, `gh[orsu]_`), Google API
      keys (`AIza`), AWS (`AKIA`), OpenAI (`sk-`), Slack (`xox*`), PEM private keys and
      certificates, JWTs, and quoted `password=` / `secret=` assignments. **Zero hits on every
      one.** A dedicated scanner is still worth running as a second opinion, not as a suspicion
- [x] **GitHub secret scanning + push protection enabled 2026-07-30**, together with Dependabot
      alerts and security updates. Not available on private repositories on the free plan, so this
      happened in the same minute as the visibility change rather than before it
- [x] ✅ **`backup-pre-squash` is gone from the repo (2026-07-30), and this is why it mattered.** A
      final sweep found **six `Co-Authored-By:` trailers naming an AI assistant** — on that branch
      and *only* that branch. `main` has zero, and the remote only ever had `main` plus tags, so
      nothing was ever exposed. But it made a single `git push --all` enough to break the owner's
      standing rule permanently, on a repo about to be public. The branch was archived to
      `git bundle` outside the repo (`verify` reports a complete history) and then deleted, so the
      footgun no longer exists rather than being documented around. `git log -p --all | grep -i
      anthropic` now returns nothing

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
      thiserror / libc / bitflags / cfg-if `MIT OR Apache-2.0`. Added 2026-07-30 for the Windows
      tray: windows-sys and windows-targets, both `MIT OR Apache-2.0`, both declarations with no
      code and no build script
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
      at all."* Windows is now stated as working, because it was run by hand on 2026-07-30; its
      *tray* is stated as built but unrun, because it is. The macOS row says "nothing — never
      built, not in CI"
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
