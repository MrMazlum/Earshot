# Earshot

**Use your phone as your PC's microphone and headphones — over Wi-Fi.**

<img src="docs/icon.png" alt="" width="88" align="right">

Free, open source, no ads, no account, no cloud. Everything stays on your home network.

> ### 🚧 Early, but it works
> On Linux, end to end: phone → Wi-Fi → PC, other applications select it as a microphone, and it has
> been used in a real Discord call. Tested on **one phone and one laptop, by one person.** The
> Windows build compiles and passes its tests in CI but has never been run by a human — see
> [What works today](#what-works-today).

---

## Download

| | File | Notes |
|---|---|---|
| 📱 **Android app** | [**earshot.apk**](https://github.com/MrMazlum/Earshot/releases/latest/download/earshot.apk) | sideload it — not on Play yet |
| 🪟 **Windows receiver** | [**earshot-receiver-windows-x86_64.zip**](https://github.com/MrMazlum/Earshot/releases/latest/download/earshot-receiver-windows-x86_64.zip) | also needs [VB-Cable](https://vb-audio.com/Cable/) — free, and Earshot walks you through it |
| 🐧 **Linux receiver** | [**earshot-receiver-linux-x86_64.tar.gz**](https://github.com/MrMazlum/Earshot/releases/latest/download/earshot-receiver-linux-x86_64.tar.gz) | nothing else to install |

> ⚠️ These links only work once this repository is public and a release is tagged; until then they
> 404 for anyone without access. CI builds all three on every push either way.

## Quick start

1. **Install the app** on your phone.
2. **Run the receiver** on your PC.
   - Windows: unzip it and double-click **`earshot.bat`**
   - Linux: `./earshot-tray` for a tray icon, or `./earshot-receiver --virtual-mic` in a terminal
3. **Type the nine-digit code** the PC shows into the app, and press Start.
4. **Pick Earshot as your microphone** in Discord, OBS, Zoom, anything.
   - Windows: the input to pick is called **`CABLE Output`**, not Earshot — see [below](#windows-and-the-virtual-cable)

That's it. Nothing to configure, nothing to sign up for.

## What it does

```
   YOU SPEAK
       ↓
   📱 phone mic  →  noise removal  →  Wi-Fi  →  💻 PC  →  "microphone"  →  Discord
                                                  ↑                          ↓
   🎧 your headset  ←  📱 phone  ←  Wi-Fi  ←──────┴───────────────  the call's audio
```

| Where | What | Job |
|---|---|---|
| 📱 Your phone | the **Earshot app** | listens to your voice, compresses it, sends it |
| 💻 Your PC | a **small background program** | receives it and hands it to Windows/Linux as a normal microphone |

Discord, Steam, OBS and everything else just see "a microphone" in their dropdown. They never know
it's your phone.

It works the other way round too: the PC's sound goes back to the phone, so you plug your headset
into the **phone** and one device does both. No wires to the PC at all.

<details>
<summary><b>Why bother? (the problem this solves)</b></summary>

Laptop microphones sound bad. They pick up the fan, the desk and your keyboard, and you're sitting
too far away from them.

Your phone is much better at this. It has several microphones and a chip whose whole job is removing
background noise — that's why you sound fine on a phone call in a noisy room.

But you can't easily use your phone as your PC's mic. Today people join the Discord call twice, once
from a second account on the phone, which causes echo and a mess of audio settings.

</details>

## The pairing code

Your PC shows something like **335 618 795**. Type those nine digits into the app. One thing to
type, instead of an IP address *and* a port number — and your network layout is not sitting on
screen in every screenshot and stream.

<details>
<summary><b>What the code is, and what it is not</b></summary>

**The code is your address in a friendlier coat, not encryption.** It is a reversible encoding, the
algorithm is in this repository, and anyone who wants the address back can have it. The receiver even
prints the address underneath the code, for the times the code doesn't work.

That's fine. It is a private address that means nothing outside your own network, and anyone already
on your network could list every device on it in about a second anyway.

What the code actually buys you is smaller, and real:

| | |
|---|---|
| Nothing to screenshot | your subnet doesn't end up in a stream or a support thread |
| Neighbouring machines look unrelated | nobody guesses the code for the PC next to yours |
| Typos are caught immediately | about 7 in 8 mistyped digits are rejected on the spot |

That last one matters more than it sounds. The alternative to "that code is not right" is a
thirty-second silent timeout — the worst error message a network app can give.

Codes cover the private address ranges and eight ports from the default. Anything else — and there
isn't much else — still works by typing the address, behind *"Type an address instead"* in the app.

</details>

## Why it feels instant

Voice chat is unusable if your words arrive late. So:

- **Wi-Fi, not Bluetooth.** Bluetooth's hands-free mode wrecks microphone quality — it's why you
  sound like a drive-thru speaker on a Bluetooth headset
- **Speed over perfection.** Audio lost on the way is skipped, never resent. Waiting for it would
  make you fall further and further behind
- **Opus**, the same codec Discord uses, at about 4 KB per second

**Target: under 100 ms** from your mouth to the other person's ear. That will be *measured* and
published here, or not claimed at all.

## The honest parts

- **Windows needs one extra free program** (VB-Cable) so the PC can treat the stream as a real
  microphone. It is closed-source, made by VB-Audio and not by us, and you install it yourself from
  their site — Earshot only detects it and links to it, and never downloads or runs anything on your
  behalf. Shipping our own would need a signed Windows driver, which costs hundreds of euros a year.
  Linux needs nothing extra
- **It only works on your own network.** No cloud, no server, no account. That's deliberate — your
  microphone should not be reachable from the internet
- **Nothing is collected.** No analytics, no crash reports, no tracking. The code is public so you
  can check that yourself
- **The pairing code identifies a PC; it does not authenticate one.** Anyone already on your network
  could send audio to a running receiver

## What works today

| | Works | Not yet |
|---|---|---|
| 📱 **Android app** | records, sends over Wi-Fi, pairs with a nine-digit code | no discovery — the code is typed, not found |
| 💻 **PC receiver** | receives, survives loss and reordering, plays out or into a virtual mic | — |
| 🖱️ **PC app** | tray icon with status, start/stop and start-at-login — no terminal | no window, no level meter |
| 🐧 **Linux** | works, virtual microphone included (`--virtual-mic`) | — |
| 🪟 **Windows** | builds and passes its tests on every push (CI, real Windows runner); finds VB-Cable and offers to install it | **never actually run by a human.** No tray icon — that is Linux-only |
| 🍎 **macOS** | nothing — never built, not in CI | not a priority. The code path exists: install BlackHole and use `--device` |

**Also missing:** Opus compression (so it currently uses ~770 kbps instead of ~40), the PC → phone
direction, automatic discovery, and encryption. Those are the next steps, in that order.

The full plan, the decisions and the open questions live in a separate project notebook, not in this
repo.

## Windows and the virtual cable

Windows cannot invent a microphone without a signed kernel driver, so Earshot borrows one. Install
[VB-Cable](https://vb-audio.com/Cable/) once — free — and Earshot plays into it. If it isn't
installed, Earshot says so, offers to open the download page, and then waits and carries on by
itself once the cable appears.

The names are back-to-front, which catches everyone out:

| | |
|---|---|
| **CABLE Input** | a *playback* device. Earshot plays into it, and finds it by itself |
| **CABLE Output** | a *recording* device. **This is the one you pick in Discord** |

There is no tray icon on Windows yet, so `earshot.bat` leaves a console window open. Keep it.

## Troubleshooting

**The app says it's sending, the PC says it's waiting.**
On Windows this is almost always the firewall: it drops incoming UDP for a program with no rule, and
the prompt that would have asked you needs an administrator. The receiver prints the fix after 25
seconds of silence — in an Administrator PowerShell:

```powershell
New-NetFirewallRule -DisplayName Earshot -Direction Inbound -Protocol UDP -LocalPort 47811 -Action Allow
```

On Linux, if you run a firewall: `sudo ufw allow 47811/udp`.

**Still nothing.** Guest Wi-Fi and "client isolation" block device-to-device traffic outright.
Turning on your phone's hotspot and joining the PC to it rules that out in a minute.

**No code appears, just an address.** Your PC is on a network outside the private ranges, or on a
VPN. Type the address and port into the app instead.

**The code is for the wrong network.** A PC on both Ethernet and Wi-Fi gets a code for each; the
receiver lists them all. Use the one for the network your phone is on.

**Windows: it runs, but Discord hears silence.** Check you picked `CABLE Output` and not `CABLE
Input`, and that both ends of the cable use the same format in Windows' Sound settings.

## Run from source

```bash
# terminal 1 — the PC side. Prints the pairing code to type into the app.
cd receiver && cargo run --release --bin earshot-receiver

# terminal 2 — a fake phone, so you can hear it working right now
cargo run --release --bin earshot-testsend -- --seconds 10
```

A 440 Hz tone should come out of your speakers, and the receiver should print a line a second saying
how much is buffered and how much was lost. To watch it cope with a bad network:

```bash
cargo run --release --bin earshot-testsend -- --loss 5 --jitter 25
```

With a real phone: `cd app && flutter build apk --release`, install it, type the pairing code, press
Start.

<details>
<summary><b>As an actual microphone, and without a terminal (Linux)</b></summary>

```bash
cargo run --release --bin earshot-receiver -- --virtual-mic
```

**Earshot** now appears in Discord's, OBS's and Zoom's input list. Nothing comes out of the speakers
in this mode — the audio goes to the virtual device instead. It stays until you reboot or run
`--remove-virtual-mic`, so applications keep remembering your choice.

```bash
cargo run --release --bin earshot-tray -- --install
```

Puts a microphone icon in the system tray and starts it at every login. Click it for the pairing
code, whether the phone is connected, and a start/stop switch. The icon changes when audio is
arriving. `--uninstall` removes the login item.

On GNOME this needs the AppIndicator extension — Ubuntu turns it on by default.

</details>

<details>
<summary><b>On Windows, from a terminal</b></summary>

```
earshot-receiver.exe --virtual-mic
```

`--list-devices` prints what it can play into, and `--device "<name>"` points it at a cable Earshot
doesn't recognise by name.

</details>

## Layout

| Folder | What's there |
|---|---|
| `app/` | the Android app — Flutter UI, Kotlin capture service |
| `receiver/` | the PC program, in Rust |
| `protocol/` | the exact format the two sides use to talk, and the pairing-code test vectors |
| `tools/` | measuring scripts |
| `docs/` | setup guides, and the checklist to clear before this repo goes public |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first — it is short, and most of it is about the audio thread
and about not committing your own IP address.

## Licence

GPL-3.0. Private for the moment; it goes public after a security review — see
`docs/pre-open-source-checklist.md`.
