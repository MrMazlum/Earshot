# Earshot RPM spec. Version is synced from receiver/Cargo.toml by CI/packaging
# scripts; when bumping the release, keep the two in step.
#
# Build locally from a checkout:
#   packaging/build-rpm.sh
# which tars the tree, drops it in ~/rpmbuild/SOURCES and calls rpmbuild.
#
# Build deps: cargo/rustc (>= 1.75), cmake, alsa-lib-devel, dbus-devel, pkgconfig.
# Runtime deps: alsa-lib, dbus-libs, and a pactl provider (pulseaudio-utils or pipewire-pulse).

Name:           earshot
Version:        1.1.0
Release:        1%{?dist}
Summary:        Use your phone as your PC's microphone, over Wi-Fi
License:        GPL-3.0-or-later
URL:            https://github.com/MrMazlum/Earshot
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo >= 1.75
BuildRequires:  cmake
BuildRequires:  gcc
BuildRequires:  alsa-lib-devel
BuildRequires:  dbus-devel
BuildRequires:  pkgconfig
Requires:       alsa-lib
Requires:       dbus-libs
# pactl is how the virtual microphone is created on Linux (see virtualmic.rs).
# Either provider satisfies it; rpm has no OR-depends, so recommend both and
# require /usr/bin/pactl weakly via Recommends where supported.
Recommends:     pulseaudio-utils
Recommends:     pipewire-pulseaudio

%description
Free, open source, no ads, no account, no cloud. Your phone sends
microphone audio over Wi-Fi; this PC receives it and exposes it as an
ordinary microphone (a PipeWire/PulseAudio source named "Earshot").
Ships earshot-receiver (terminal) and earshot-tray (system tray;
start-at-login with `earshot-tray --install`).

%prep
%autosetup -n %{name}-%{version}

%build
cd receiver
cargo build --release --locked --offline || cargo build --release --locked

%install
install -Dm0755 receiver/target/release/earshot-receiver %{buildroot}%{_bindir}/earshot-receiver
install -Dm0755 receiver/target/release/earshot-tray %{buildroot}%{_bindir}/earshot-tray
install -Dm0644 packaging/earshot.desktop %{buildroot}%{_datadir}/applications/earshot.desktop
install -Dm0644 docs/icon.png %{buildroot}%{_datadir}/icons/hicolor/512x512/apps/earshot.png
install -Dm0644 LICENSE %{buildroot}%{_docdir}/%{name}/copyright

%files
%{_bindir}/earshot-receiver
%{_bindir}/earshot-tray
%{_datadir}/applications/earshot.desktop
%{_datadir}/icons/hicolor/512x512/apps/earshot.png
%license LICENSE
%doc README.md CHANGELOG.md

%changelog
* Thu Nov 05 2026 Mazlum <https://github.com/MrMazlum/Earshot> - 1.1.0-1
- First packaged release (deb/rpm/AppImage/AUR).
