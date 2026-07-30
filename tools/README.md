# tools

Development helpers. Never shipped in a release artifact.

- `icon/make_icons.py` — generates the app icon and every Android launcher size from one script,
  so the repository carries the recipe rather than a pile of PNGs nobody can regenerate

## Not written yet

The two measurements the project most needs, kept here as a note of what is missing rather than as
an empty directory pretending to be code:

- **A latency probe.** Nothing in this repository has ever measured glass-to-glass latency. Until
  something does, the README's "target: under 100 ms" stays a target and is never stated as a
  result — see the measurement rule in [CONTRIBUTING.md](../CONTRIBUTING.md)
- **A microphone comparison harness.** The app's mic-source picker offers `VOICE_COMMUNICATION` and
  `MIC` because those two differ by Android specification. Whether they differ *audibly*, and by how
  much, is unproven; the other three sources stay locked until it is

For sending audio without a phone, use `earshot-testsend` in `receiver/` instead — it is a
first-class binary, not a tool.
