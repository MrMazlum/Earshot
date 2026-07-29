# Pre-open-source checklist

This repo is **private** until every box below is ticked. The plan is to publish under GPL-3.0.
Owner's instruction (2026-07-29): *"after all of it is done we will make sure there is no security
leak and then we will make it open sourced."*

## 1. History, not just the working tree
Deleting a secret in a later commit does not remove it — it stays in the git history and in every
clone. Scan the **whole history**, and if anything is found, rewrite it before publishing (or start
the public repo from a fresh, squashed initial commit).

- [x] **History squashed to a single commit (2026-07-29).** Everything before it is gone, so the
      only thing left to scan is the working tree
- [ ] Scan full history with a secret scanner (e.g. `gitleaks detect`, `trufflehog`)
- [ ] `git log -p | grep -iE 'token|secret|password|api[_-]?key|BEGIN .*PRIVATE KEY'` — manual pass
- [ ] Enable GitHub secret scanning + push protection on the repo before it goes public

## 2. Credentials and signing
- [ ] No Android keystore, `key.properties`, or upload key in the tree or history
- [ ] No `.env`, no service-account JSON, no API tokens
- [ ] If anything was ever committed: **rotate it**, do not just delete it

## 3. Personal and network data
- [ ] No home IP addresses, MAC addresses, SSIDs, or router details in code, docs, logs or test vectors
- [ ] No device serial numbers (ADB IDs) committed
- [ ] No voice recordings from P0.1 in the repo — they live in `~/EarshotBrain/raw/`, which is not published
- [ ] No screenshots or logs containing the LAN topology
- [ ] Decide explicitly whether the project brain (`~/EarshotBrain/`) is published; by default it is **not**
- [x] No absolute local paths containing the owner's username in tracked files. The file that had
      them is no longer committed; `CONTRIBUTING.md` replaced it. Source comments still point at
      the project notebook by name — check those before publishing
- [x] Pairing-code test vectors use made-up addresses only (`protocol/pairing-vectors.csv`)

## 4. Security review of the code itself
Publishing invites people to look for holes — better to find them first.

- [ ] **Packet parser**: every field bounds-checked; malformed/oversized/truncated datagrams from anyone on the LAN cannot crash or over-read the receiver
- [ ] **Pairing**: the nine-digit code *identifies* a PC, it does not authenticate one — it is a
      reversible encoding of the address and nothing more (`receiver/src/pairing.rs` says so at the
      top). Anyone already on the LAN can still send audio to a running receiver. Decide before
      publishing whether that is acceptable or whether P6 must land first
- [ ] **Encryption**: is the audio payload encrypted, and is the debug/plaintext mode compiled out of release builds?
- [ ] **Binding**: does the receiver listen only on the intended interface, not 0.0.0.0 by accident?
- [ ] **Dependencies**: `cargo audit` clean; every dependency licence-checked and GPL-3.0-compatible
- [ ] **Privacy claim verified**: prove there is no outbound connection beyond the LAN — the README claims it, so it must survive someone checking

## 5. Licence and attribution
- [ ] `LICENSE` present and correct; headers where the project convention requires them
- [ ] Third-party licences acknowledged (libopus, Oboe, crates)
- [x] The Windows VB-Cable dependency disclosed in the README as a closed-source third-party
      component the user installs themselves ("The honest parts")
- [x] **The guided install never fetches or executes anything.** `receiver/src/cable.rs` detects a
      missing cable, explains it, and opens VB-Audio's own page in the browser. Downloading the
      driver pack and elevating it for the user was considered and rejected: it is a third-party
      kernel driver, there is no published checksum to verify a download against, and teaching users
      to accept a silent elevated install is a bad habit to build into a tool. If that ever changes,
      it needs a checksum shown to the user and an explicit opt-in flag

## 6. Publication hygiene
- [ ] README honest about status and about **measured** latency (no unmeasured claims)
- [ ] Issue templates that do not ask users for network dumps containing their own IP
- [ ] `SECURITY.md` with a contact for reporting vulnerabilities
- [x] Squash or review the pre-public commit history for anything embarrassing or identifying
