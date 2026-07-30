# Earshot

**Use your phone as your PC's microphone — over Wi-Fi.**

<img src="docs/icon.png" alt="" width="88" align="right">

Free, open source, no ads, no account, no cloud. Everything stays on your home network.

> ### 🚧 Early, but it works
> End to end on **Linux and Windows**: phone → Wi-Fi → PC, other applications select it as a
> microphone, and it has been used in a real Discord call. Tested on **one phone and two PCs, by one
> person** — see [What works today](#what-works-today). Nothing about the latency has been
> *measured* yet, and nothing is claimed about it.

---

## Download

| | File | Notes |
|---|---|---|
| 📱 **Android app** | [**earshot.apk**](https://github.com/MrMazlum/Earshot/releases/latest/download/earshot.apk) | sideload it — not on Play yet |
| 🪟 **Windows receiver** | [**earshot-receiver-windows-x86_64.zip**](https://github.com/MrMazlum/Earshot/releases/latest/download/earshot-receiver-windows-x86_64.zip) | also needs [VB-Cable](https://vb-audio.com/Cable/) — free, once, [and there is a right way to install it](#setup) |
| 🐧 **Linux receiver** | [**earshot-receiver-linux-x86_64.tar.gz**](https://github.com/MrMazlum/Earshot/releases/latest/download/earshot-receiver-linux-x86_64.tar.gz) | nothing else to install |

> ⚠️ These links only work once this repository is public and a release is tagged; until then they
> 404 for anyone without access. CI builds all three on every push either way.

## Setup

<details open>
<summary><b>🪟 Windows</b> — four steps, once</summary>

**1. Install VB-Cable.** Windows will not let a program invent a microphone — that needs a driver
signed by Microsoft — so Earshot borrows one. This is the only awkward part, and you do it once.

Go to **[vb-audio.com/Cable](https://vb-audio.com/Cable/)** and find the box titled
**"VB-CABLE Driver Pack"**. The file is `VBCABLE_Driver_Pack45.zip`, dated **2024**.

> ⚠️ That page advertises five products at once. **Do not** take *Hi-Fi Cable & ASIO Bridge* — it is
> dated **2014**, it sits near the top, and it looks like the safe established choice. It is a
> different product. Skip *VB-CABLE A+B* and *C+D* too. You want the plain Driver Pack.

Extract the zip. There are **two** installers inside, and nothing in the zip says which is which:

| | |
|---|---|
| `VBCABLE_Setup_x64.exe` | **this one** — any PC from the last 15 years |
| `VBCABLE_Setup.exe` | only for 32-bit Windows |

**Right-click it → "Run as administrator".** Double-clicking it looks like it worked and installs
nothing: Windows will not add a driver without administrator rights, and it does not tell you that.
Then "Install Driver", accept the security prompt, reboot if it asks.

**2. Run Earshot.** Unzip the release and double-click **`Earshot.exe`**. No window opens — a
microphone icon appears next to the clock, and a notification shows your pairing code. You may have
to click the `^` arrow to see the icon; drag it onto the taskbar to keep it there.

| Icon | |
|---|---|
| ⚪ grey | stopped |
| 🔵 blue | running, waiting for your phone |
| 🟢 green | your voice is arriving right now |
| 🔴 red | something is wrong — click it |

**3. The phone.** Install the APK, type the nine digits, press Start. Android will warn you that the
developer is unknown — [it's right, and here's how to get past it](#your-phone-will-warn-you).

**4. Pick it in Discord.** In the microphone list choose **`CABLE Output`** — *not* `CABLE Input`.
[Why it isn't called Earshot, and how to rename it.](#why-your-microphone-is-called-cable-output)

The zip also has a `START HERE.txt` with all of the above, and the tray menu has
*"How do I use this?"* and *"It is not working..."* entries with the same advice.

</details>

<details open>
<summary><b>🐧 Linux</b> — two steps, nothing to install</summary>

**1.** Extract the release and run `./earshot-tray`. A microphone icon appears in the tray with your
pairing code in its menu. Or `./earshot-receiver --virtual-mic` if you want a terminal and live
statistics.

**2.** Install the APK on your phone, type the nine digits, press Start. In Discord, OBS or Zoom,
pick **Earshot** in the input list.

`./earshot-tray --install` copies it to `~/.local/bin` and starts it at every login; `--uninstall`
undoes that. On GNOME the tray needs the AppIndicator extension, which Ubuntu enables by default.

</details>

### Your phone will warn you

The app is not on the Play Store and is not signed by a registered developer, so Android says so —
twice. That warning is correct, and it appears for every sideloaded app.

1. **While downloading**, Chrome may say the file "may be harmful". Choose **Download anyway** /
   **Keep**.
2. **While installing**, Play Protect says *"Blocked by Play Protect"* or *"unknown developer"*. The
   big obvious button is **Don't install**. Choose **More details → Install anyway**.

If you would rather not take our word for it, the source is right here and
`cd app && flutter build apk --release` produces the same thing.

### Why your microphone is called `CABLE Output`

Because it is VB-Audio's device, not ours. On Linux, Earshot creates its own input and calls it
**Earshot**. On Windows it cannot: publishing an audio endpoint needs a kernel driver signed by
Microsoft, so Earshot plays into a cable somebody else already signed.

The two ends read backwards, which catches out everybody:

| | |
|---|---|
| **CABLE Input** | a *playback* device. Earshot plays into it, and finds it by itself |
| **CABLE Output** | a *recording* device. **This is the one you pick in Discord** |

**You can rename it.** Click the Earshot tray icon → *"Rename this input to Earshot..."*. It opens
the exact dialog for you and tells you which box to type in. Windows remembers the new name and
every application picks it up (Discord may need restarting first).

Earshot does not rename it for you on purpose: the name lives in `HKEY_LOCAL_MACHINE`, so writing it
would mean elevating to administrator in order to edit another vendor's driver settings — for a
cosmetic change you can make yourself in fifteen seconds.

## What it does

```
   YOU SPEAK
       ↓
   📱 phone mic  →  noise removal  →  Wi-Fi  →  💻 PC  →  "microphone"  →  Discord
```

| Where | What | Job |
|---|---|---|
| 📱 Your phone | the **Earshot app** | listens to your voice and sends it |
| 💻 Your PC | a **small background program** | receives it and hands it to Windows/Linux as a normal microphone |

Discord, Steam, OBS and everything else just see "a microphone" in their dropdown. They never know
it's your phone.

**Planned, and not built yet:** the return direction, so the call's audio goes back to the phone and
you plug your headset into the **phone** instead — one device for both, no wires to the PC at all.
Today Earshot is a microphone only, and the sound still comes out of whatever the PC uses now.

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
- **Small, fixed buffers**, and the delay they cost is on screen while it runs, in milliseconds

Compression is the gap: it currently sends raw audio at about 770 kbps. **Opus** — the same codec
Discord uses, at roughly 4 KB per second — is the next thing to land, and it is not in yet.

**Target: under 100 ms** from your mouth to the other person's ear. That will be *measured* and
published here, or not claimed at all. It has not been measured, so it is not claimed.

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
- **The audio is not encrypted yet.** It crosses your own Wi-Fi as plain samples. [SECURITY.md](SECURITY.md)
  lists what Earshot does and does not promise, and how to report a problem privately

## What works today

| | Works | Not yet |
|---|---|---|
| 📱 **Android app** | records, sends over Wi-Fi, pairs with a nine-digit code | no discovery — the code is typed, not found |
| 💻 **PC receiver** | receives, survives loss and reordering, plays out or into a virtual mic | — |
| 🖱️ **PC app** | tray icon on both platforms: pairing code, start/stop, start-at-login — no terminal | no window, no level meter |
| 🐧 **Linux** | works, virtual microphone included (`--virtual-mic`) | — |
| 🪟 **Windows** | **works** — tested by hand 2026-07-30: audio arrives, VB-Cable path, tray icon | the input is VB-Audio's `CABLE Output` unless you rename it |
| 🍎 **macOS** | nothing — never built, not in CI | not a priority. The code path exists: install BlackHole and use `--device` |

**Also missing:** Opus compression (so it currently uses ~770 kbps instead of ~40), the PC → phone
direction, automatic discovery, and encryption. Those are the next steps, in that order.

The full plan, the decisions and the open questions live in a separate project notebook, not in this
repo.

## Troubleshooting

**The app says it's sending, the tray icon stays blue.**
On Windows this is almost always the firewall: it drops incoming UDP for a program with no rule, and
the prompt that would have asked you needs an administrator, so it is often never shown at all. The
tray's *"It is not working..."* entry has this, and the console receiver prints it after 25 seconds
of silence. In an Administrator PowerShell:

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

**Windows: the icon is green, but Discord hears silence.** You picked `CABLE Input` instead of
`CABLE Output`. If it is definitely the Output, check both ends of the cable are set to the same
format in Windows' Sound settings.

**Windows: the tray icon has disappeared.** Windows hides tray icons by default — click the `^`
arrow next to the clock, then drag the microphone icon out onto the taskbar so it stays.

**Windows: nothing happens when I double-click `Earshot.exe`.** It has no window by design; look
next to the clock. If it is not there either, run `earshot-console.bat` instead, which keeps a
terminal open and will say what went wrong.

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
<summary><b>As an actual microphone, and without a terminal</b></summary>

```bash
cargo run --release --bin earshot-receiver -- --virtual-mic
```

**Earshot** now appears in Discord's, OBS's and Zoom's input list. Nothing comes out of the speakers
in this mode — the audio goes to the virtual device instead. It stays until you reboot or run
`--remove-virtual-mic`, so applications keep remembering your choice.

```bash
cargo run --release --bin earshot-tray -- --install
```

Puts a microphone icon in the system tray and starts it at every login — on Windows too, where the
same binary ships as `Earshot.exe`. Click it for the pairing
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
| `protocol/` | [the wire format, specified](protocol/README.md), and the pairing-code test vectors |
| `tools/` | development helpers |
| `docs/` | the checklist to clear before this repo goes public |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first — it is short, and most of it is about the audio thread
and about not committing your own IP address. Writing another client? Everything you need is in
[`protocol/README.md`](protocol/README.md).

Found a security problem? Please report it privately — see [SECURITY.md](SECURITY.md).

## Licence

GPL-3.0. Private for the moment; it goes public after a security review — see
`docs/pre-open-source-checklist.md`.
