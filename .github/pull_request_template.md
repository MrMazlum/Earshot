<!-- Short is fine. These four questions exist because of how this project breaks, not for form's sake. -->

## What this changes

## Latency cost

<!-- Required if this adds or grows any buffer, queue or wait: state the cost in milliseconds.
     "None" is a perfectly good answer. 150 ms glass-to-glass is a hard cap. -->

None.

## Checks

- [ ] `cd receiver && cargo test --locked && cargo clippy --locked --all-targets -- -D warnings`
- [ ] `cd app && flutter test && flutter analyze` (if the app or the pairing code changed)
- [ ] Nothing new is printed to a terminal in anything but plain ASCII
- [ ] No IP address, MAC address, SSID, device serial, keystore or token anywhere in the diff
- [ ] The wire format is unchanged — or `protocol/README.md`, both implementations and the test
      vectors all changed together, in this one commit
