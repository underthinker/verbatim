---
title: Install
description: Install Verbatim on macOS, Windows, or Linux from the channel that suits you - Homebrew, winget, Flathub, AppImage, or a direct download.
---

Verbatim ships signed builds for macOS, Windows 11, and Ubuntu 24.04 (and most modern Linux desktops).
Pick the channel you prefer below.
After installing, continue to [Permissions](/permissions/) - Verbatim needs a couple of one-time grants before it can hear you and type for you.

## macOS

Verbatim supports Apple Silicon and Intel Macs.

**Homebrew (recommended):**

```sh
brew install --cask verbatim
```

**Direct download:**
Download the `.dmg` from the [latest release](https://github.com/underthinker/verbatim/releases/latest), open it, and drag Verbatim to Applications.

The build is signed with an Apple Developer ID and notarized, so Gatekeeper opens it without a warning.

## Windows

Windows 11 x64 is supported.

**winget (recommended):**

```powershell
winget install Verbatim
```

**Direct download:**
Download the `.msi` from the [latest release](https://github.com/underthinker/verbatim/releases/latest) and run it.

The installer is Authenticode-signed.
On a brand-new signing certificate, SmartScreen may still show a "Windows protected your PC" prompt the first few times - click **More info -> Run anyway**.

## Linux

**Flathub (recommended for GNOME/KDE):**

```sh
flatpak install flathub app.verbatim.Verbatim
```

**AppImage:**
Download the `.AppImage` from the [latest release](https://github.com/underthinker/verbatim/releases/latest), make it executable, and run it:

```sh
chmod +x Verbatim-*.AppImage
./Verbatim-*.AppImage
```

**Debian/Ubuntu (`.deb`):**

```sh
sudo apt install ./verbatim_*.deb
```

### One extra step for typing on Linux

To type into other apps, Verbatim uses the desktop's input portal where available, and otherwise falls back to `/dev/uinput`.
The `uinput` fallback needs a one-time permission grant.
Both the AppImage and Flatpak bundle a helper script for this, and Verbatim's onboarding walks you through it (this is the [E9](/troubleshooting/#e9---verbatim-needs-permission-to-type-on-linux) guided setup).
See [Permissions -> Linux](/permissions/#linux) for the details.

## Verifying a download

Every release publishes SHA-256 checksums and an SBOM alongside the artifacts.
To verify a file you downloaded manually:

```sh
# macOS / Linux
shasum -a 256 <downloaded-file>
# Windows (PowerShell)
Get-FileHash <downloaded-file> -Algorithm SHA256
```

Compare the output against the checksum listed on the release page.
