---
title: Install
description: Install Verbatim on macOS, Windows, or Linux from the channel that suits you - Homebrew, winget, Flathub, AppImage, or a direct download.
---

Verbatim builds for macOS, Windows 11, and Ubuntu 24.04 (and most modern Linux desktops).
Pick the channel you prefer below.
After installing, continue to [Permissions](/permissions/) - Verbatim needs a couple of one-time grants before it can hear you and type for you.

:::caution[Verbatim's builds are not code-signed]
Verbatim is a small local-first project and does not pay for an Apple Developer ID or a Windows Authenticode certificate.
The binaries are built in public from tagged commits by [GitHub Actions](https://github.com/underthinker/verbatim/actions), and every release publishes SHA-256 checksums you can verify yourself.
The trade-off is that macOS and Windows each show a one-time warning you have to click past.
Both are covered below.
:::

## macOS

Verbatim supports Apple Silicon and Intel Macs.

**Homebrew (recommended):**

```sh
brew install --cask --no-quarantine verbatim
```

`--no-quarantine` tells macOS not to flag the app as downloaded-from-the-internet, which is what makes Gatekeeper refuse to open an unsigned build.
Without it, you will need the "Open Anyway" steps below.

**Direct download:**
Download the `.dmg` from the [latest release](https://github.com/underthinker/verbatim/releases/latest), open it, and drag Verbatim to Applications.

The first launch will fail with *"Verbatim" is damaged and can't be opened* or *cannot be opened because Apple could not verify it*.
That message is Gatekeeper reacting to the missing signature, not a corrupted download.
Clear it once, either way:

```sh
xattr -dr com.apple.quarantine /Applications/Verbatim.app
```

Or, without the terminal: open Verbatim once and dismiss the warning, then go to **System Settings -> Privacy & Security**, scroll to the message about Verbatim being blocked, and click **Open Anyway**.

:::note
Because the build is unsigned, macOS identifies Verbatim by the exact bytes of the app rather than by a signing certificate.
Every time you install a new version, macOS treats it as a brand-new app and asks you to grant Microphone and Accessibility permission again.
This is expected, and it is the main day-to-day cost of skipping the certificate.
:::

## Windows

Windows 11 x64 is supported.

**winget (recommended):**

```powershell
winget install Verbatim
```

**Direct download:**
Download the `.msi` from the [latest release](https://github.com/underthinker/verbatim/releases/latest) and run it.

Because the installer is unsigned, SmartScreen shows a **"Windows protected your PC"** dialog.
Click **More info**, confirm the publisher line reads *Unknown publisher*, then click **Run anyway**.

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
