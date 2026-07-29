# Earshot

**Use your phone as your PC's microphone and headphones — over Wi-Fi.**

Free, open source, no ads, no account. Everything stays on your home network.

> ### 🚧 Early, but it works
> On Linux, end to end: phone → Wi-Fi → PC, other applications select it as a microphone, and it
> has been used in a real Discord call. Tested on one phone and one laptop, by one person. The
> Windows build compiles and passes its tests in CI but **has never been run by a human**. See
> [Where the project is](#where-the-project-is).

<img src="docs/icon.png" alt="" width="96" align="right">

## Download

| | | |
|---|---|---|
| 📱 **Android app** | [**earshot.apk**](https://github.com/MrMazlum/Earshot/releases/latest/download/earshot.apk) | sideload it — not on Play yet |
| 🪟 **Windows receiver** | [**earshot-receiver-windows-x86_64.zip**](https://github.com/MrMazlum/Earshot/releases/latest/download/earshot-receiver-windows-x86_64.zip) | also needs [VB-Cable](https://vb-audio.com/Cable/), free |
| 🐧 **Linux receiver** | [**earshot-receiver-linux-x86_64.tar.gz**](https://github.com/MrMazlum/Earshot/releases/latest/download/earshot-receiver-linux-x86_64.tar.gz) | nothing else to install |

Install the app on the phone, run the receiver on the PC, type the nine-digit code the receiver
prints. [Full instructions below.](#try-it)

> **These links only work once the repository is public and a release is tagged.** Until then they
> return 404 for anyone without access to the repo — see [Licence](#licence). Both receivers and the
> APK are built by CI on every push, so the artifacts exist either way.

---

## The problem

Laptop microphones sound bad. They pick up the fan, the desk and your keyboard, and you're sitting
too far away from them.

Your phone is much better at this. It has several microphones and a chip whose whole job is removing
background noise — that's why you sound fine on a phone call in a noisy room.

But you can't easily use your phone as your PC's mic. Today people join the Discord call twice, once
from a second account on the phone, which causes echo and a mess of audio settings.

## The idea

Two pieces that talk to each other over your Wi-Fi:

| Where | What | What it does |
|-------|------|--------------|
| 📱 Your phone | the **Earshot app** | listens to your voice, compresses it, sends it |
| 💻 Your PC | a **small background program** | receives it and hands it to Windows/Linux as a normal microphone |

Discord, Steam, OBS and everything else just see "a microphone" in their dropdown. They never know
it's your phone.

It works the other way round too: the PC's sound is sent back to the phone, so you plug your headset
into the **phone** and one device does both. No wires to the PC at all.

```
   YOU SPEAK
       ↓
   📱 phone mic  →  noise removal  →  Wi-Fi  →  💻 PC  →  "microphone"  →  Discord
                                                  ↑                          ↓
   🎧 your headset  ←  📱 phone  ←  Wi-Fi  ←──────┴───────────────  the call's audio
```

## How you use it

1. Install the app on your phone
2. Run the small Earshot program on your PC
3. Type into the phone the nine-digit code your PC is showing
4. In Discord, pick "Earshot" as your microphone

On Windows, step 4 needs one extra free install — see [the honest parts](#the-honest-parts).

### About that code

Your PC shows something like **335 618 795**. That is one thing to type instead of an IP address
*and* a port number, and it means your network layout is not sitting on screen in every screenshot
and stream.

Be clear about what it is, though: **the code is your address in a friendlier coat, not
encryption.** It is a reversible encoding and the algorithm is in this repository, so anyone who
wants the address back can have it. That is fine — it is a private address that means nothing
outside your own network, and anyone already on your network could list every device on it in about
a second anyway. What the code actually buys you is smaller and real: nothing to screenshot,
neighbouring machines produce codes that look unrelated so nobody guesses along a subnet, and about
seven out of eight mistyped digits are rejected on the spot instead of becoming a connection that
never arrives.

Codes cover the private address ranges and eight ports from the default. Anything else — and there
is not much else — still works by typing the address, behind *"Type an address instead"* in the
app.

## Why it should feel instant

Voice chat is unusable if your words arrive late. So:

- **Wi-Fi, not Bluetooth.** Bluetooth's hands-free mode wrecks microphone quality — it's why you
  sound like a drive-thru speaker on a Bluetooth headset
- **Speed over perfection.** If a piece of audio gets lost on the way it is skipped, never resent.
  Waiting for it would make you fall further and further behind
- **Opus**, the same codec Discord uses, at about 4 KB per second

**Target: under 100 ms** from your mouth to the other person's ear. That will be *measured* and
published here, or not claimed at all.

## The honest parts

- **Windows will probably need one extra free program** (VB-Cable) so the PC can treat the stream as
  a real microphone. Shipping our own would need a signed Windows driver, which costs hundreds of
  euros a year. Linux needs nothing extra
- **It only works on your own network.** No cloud, no server, no account. That's deliberate — your
  microphone should not be reachable from the internet
- **Nothing is collected.** No analytics, no crash reports, no tracking. The code is public so you
  can check that yourself

## Where the project is

| | Works today | Not yet |
|---|---|---|
| 📱 **Android app** | records, sends over Wi-Fi, pairs with a nine-digit code | no discovery — the code is typed, not found |
| 💻 **PC receiver** | receives, survives loss and reordering, plays out or into a virtual mic | — |
| 🖱️ **PC app** | a tray icon with status, start/stop and start-at-login — no terminal | no window, no level meter |
| 🐧 **Linux** | works, including the virtual microphone (`--virtual-mic`) | — |
| 🪟 **Windows** | builds and passes its tests on every push (CI, real Windows runner); `--virtual-mic` finds VB-Cable | **never actually run by a human.** No tray icon either — that is Linux-only |
| 🍎 **macOS** | same | not a priority |

Also missing: Opus compression (so it currently uses ~770 kbps instead of ~40), the PC → phone
direction, automatic discovery, and encryption. Those are the next steps in order. The pairing code
identifies a PC; it does not yet authenticate one, so anyone already on your network could send
audio to a running receiver.

The full plan, the decisions and the open questions live in a separate project notebook, not in
this repo.

## Try it

```bash
# terminal 1 — the PC side. Prints the pairing code to type into the app.
cd receiver && cargo run --release --bin earshot-receiver

# terminal 2 — a fake phone, so you can hear it working right now
cargo run --release --bin earshot-testsend -- --seconds 10
```

A 440 Hz tone should come out of your speakers, and the receiver should print a line a second
saying how much is buffered and how much was lost. To see it cope with a bad network:

```bash
cargo run --release --bin earshot-testsend -- --loss 5 --jitter 25
```

With a real phone: build the app (`cd app && flutter build apk --release`), install it, type the
nine-digit pairing code the receiver printed, press Start.

### Using it as an actual microphone (Linux)

```bash
cargo run --release --bin earshot-receiver -- --virtual-mic
```

Now **Earshot** appears in Discord's, OBS's and Zoom's input list. Nothing comes out of the
speakers in this mode — the audio goes to the virtual device instead. It stays until you reboot or
run `--remove-virtual-mic`, so applications keep remembering your choice.

### Without a terminal (Linux)

```bash
cargo run --release --bin earshot-tray -- --install
```

Puts a microphone icon in the system tray and starts it at every login. Click it for the pairing
code to type into the phone, whether the phone is connected, and a start/stop switch. The icon changes when
audio is arriving. `--uninstall` undoes the login item.

On GNOME this needs the AppIndicator extension — Ubuntu turns it on by default.

### On Windows

```
earshot-receiver.exe --virtual-mic
```

Windows cannot invent a microphone without a signed kernel driver, so Earshot borrows one:
install [VB-Cable](https://vb-audio.com/Cable/) once, and Earshot plays into it. The names are
back-to-front, which catches everyone out:

- Earshot plays into **CABLE Input** — it finds this by itself
- **you pick CABLE Output** as your microphone in Discord

There is no tray icon on Windows yet, so this needs a terminal window left open.

## Layout

| Folder | What is there |
|--------|---------------|
| `app/` | the Android app — Flutter UI, Kotlin capture service |
| `receiver/` | the PC program, in Rust |
| `protocol/` | the exact format the two sides use to talk to each other, and the pairing-code test vectors |
| `tools/` | measuring scripts |
| `docs/` | setup guides, and the checklist to clear before this repo goes public |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first — it is short, and most of it is about the audio
thread and about not committing your own IP address.

## Licence

GPL-3.0. Private for the moment; it goes public after a security review — see
`docs/pre-open-source-checklist.md`.
