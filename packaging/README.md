# Linux packaging

One source of truth for the version: `receiver/Cargo.toml`. Every script
below reads it — never hard-code a version in a second place.

| Artifact | Built by | Contents |
|---|---|---|
| 1-line install | `install.sh` (repo root) | latest GitHub release tarball → `$PREFIX/bin`, menu entry, icon |
| `.deb` | `packaging/build-deb.sh` | `earshot-receiver` + `earshot-tray`, `.desktop`, icon, docs |
| `.rpm` | `packaging/build-rpm.sh` + `packaging/earshot.spec` | same, FHS paths |
| `.AppImage` | `packaging/build-appimage.sh` | portable bundle, runs `earshot-tray` by default |
| AUR `-git` | `packaging/aur/earshot-git/PKGBUILD` | **always the latest commit** — rebuilds `main` on every install |
| AUR `-bin` | `packaging/aur/earshot-bin/PKGBUILD` | prebuilt release binaries, no compile |

## Local builds

```sh
# .deb (needs dpkg-deb; builds the binaries first with cargo --locked)
./packaging/build-deb.sh --output-dir dist/

# .rpm (needs rpmbuild)
./packaging/build-rpm.sh --output-dir dist/

# .AppImage (needs curl; fetches appimagetool if missing)
./packaging/build-appimage.sh --output-dir dist/

# 1-line install, user scope (no root):
./install.sh
# system-wide:
sudo ./install.sh
# remove:
sudo ./install.sh --uninstall
```

Debian `Depends` is `libasound2t64 | libasound2, libdbus-1-3,
pulseaudio-utils | pipewire-pulse` — `pactl` is how the virtual microphone
is created on Linux (`receiver/src/virtualmic.rs`). RPM mirrors this with
`Recommends`. The tray needs an AppIndicator on stock GNOME (Ubuntu ships
it by default).

## Which AUR package?

- **`earshot-git` — latest, every time.** It clones `main` and compiles it,
  with `pkgver()` derived from `Cargo.toml` + commit count, so every
  reinstall (`yay -S earshot-git`) builds the newest commit. This is the
  answer to "I want the latest version every time I install from AUR".
- **`earshot-bin` — pinned release, no compile.** Downloads the
  `v<version>` tarball from GitHub releases. Fast, but only moves when the
  maintainer bumps `pkgver` after a release.

```sh
# latest commit, compiled locally (heavy: DeepFilterNet, ~minutes)
yay -S earshot-git
# or: paru -S earshot-git / git clone + makepkg -si

# pinned release binary, instant
yay -S earshot-bin
```

## Publishing to AUR (maintainer)

Each AUR package is its own tiny git repo on `aur.archlinux.org`, seeded
from the matching directory in this repo. First time only:

```sh
# 1. Create the empty packages on https://aur.archlinux.org
#    (Packages -> Submit: earshot-git, earshot-bin).
# 2. Add your AUR ssh key to your account, then:
git clone ssh://aur@aur.archlinux.org/earshot-git.git /tmp/aur-git
cp packaging/aur/earshot-git/PKGBUILD packaging/aur/earshot-git/.SRCINFO /tmp/aur-git/
cd /tmp/aur-git && makepkg --printsrcinfo > .SRCINFO && git add PKGBUILD .SRCINFO && git commit -m "init" && git push

git clone ssh://aur@aur.archlinux.org/earshot-bin.git /tmp/aur-bin
cp packaging/aur/earshot-bin/PKGBUILD packaging/aur/earshot-bin/.SRCINFO /tmp/aur-bin/
cd /tmp/aur-bin && makepkg --printsrcinfo > .SRCINFO && git add PKGBUILD .SRCINFO && git commit -m "init" && git push
```

On every release (`vX.Y.Z` tag), bump `-bin`:

```sh
# update pkgver in packaging/aur/earshot-bin/PKGBUILD + .SRCINFO, then push it:
cd /tmp/aur-bin
cp ~/Earshot/packaging/aur/earshot-bin/* .
makepkg --printsrcinfo > .SRCINFO   # regenerates pkgver metadata
git commit -am "vX.Y.Z" && git push
```

`-git` needs no bump ever: its `pkgver()` computes the version at build
time, so users get the latest commit without any maintainer action.

## CI

`.github/workflows/packaging.yml` runs on every `v*` tag: it builds the
`.deb`, `.rpm` and `.AppImage` from source and uploads them to the GitHub
release alongside the tarballs, and it syntax-checks both PKGBUILDs.
