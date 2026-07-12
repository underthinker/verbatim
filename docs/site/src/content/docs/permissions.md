---
title: Permissions
description: The one-time system permissions Verbatim needs per operating system - microphone access everywhere, plus Accessibility on macOS and typing access on Linux.
---

Verbatim needs two kinds of permission: one to **hear** you (the microphone) and one to **type** for you (input injection).
Verbatim's onboarding requests these on first run and re-checks them live, so you rarely need this page - but here is exactly what each grant is and how to fix it if it gets revoked.

Verbatim asks for nothing else.
It never requests network, contacts, files, or location.

## Microphone (all platforms)

Verbatim can't transcribe what it can't hear.
The first time you dictate, your OS prompts for microphone access.
If you declined, or it was turned off later, you'll see [E1 - "Verbatim can't hear you yet"](/troubleshooting/#e1---verbatim-cant-hear-you-yet).

- **macOS:** System Settings -> Privacy & Security -> Microphone -> enable Verbatim.
- **Windows:** Settings -> Privacy & security -> Microphone -> enable Verbatim (and make sure "Let apps access your microphone" is on).
- **Linux:** microphone access is not gated separately on most desktops; if you use a sandboxed Flatpak, allow audio input in your desktop's Flatpak permissions (or via Flatseal).

## macOS - Accessibility

To type into other apps, macOS requires Verbatim to be trusted for **Accessibility**.
Onboarding prompts you, or you can grant it at:

System Settings -> Privacy & Security -> Accessibility -> enable Verbatim.

If typing silently fails and your text lands on the clipboard instead, Accessibility is the usual cause - see [E4](/troubleshooting/#e4---couldnt-type-into-this-app).

Verbatim deliberately does **not** run in the App Sandbox, because sandboxed apps can't use Accessibility to type into other apps.

Because the build is unsigned, macOS keys these two grants to the exact bytes of the app rather than to a signing certificate.
Installing a new version therefore clears both grants, and Verbatim asks for them again on the next dictation.
The stale entries left behind in System Settings are harmless; you can remove them with the **-** button.

### Input Monitoring for a modifier-only hotkey

An ordinary shortcut such as **Ctrl + Alt + Space** does not need Input Monitoring.
A bare right-side modifier such as **Right Option** does, because Verbatim must listen for that key's press and release events.
Enable it under System Settings -> Privacy & Security -> Input Monitoring, then restart Verbatim.
Unsigned rebuilds must be enabled again after each install for the same identity reason described above.
If Verbatim is already shown as enabled but the hotkey does nothing, select its stale row, click **-**, click **+**, add `/Applications/Verbatim.app` again, and restart the app.
Toggling a stale row off and on does not update the code identity macOS stored with it.

## Windows

Typing works out of the box - no extra permission is needed for ordinary apps.

Two things to know:

- Apps running **as administrator** are protected by Windows UIPI, so a non-elevated Verbatim can't type into them.
  Run Verbatim with the same elevation as the target app if you need to dictate into an elevated window.
- If typing fails, your text is placed on the clipboard so you can paste it - see [E4](/troubleshooting/#e4---couldnt-type-into-this-app).

## Linux

Verbatim tries to type in this order:

1. The desktop's **input portal** (libei / RemoteDesktop) - preferred on Wayland (GNOME, KDE Plasma 6), no special permission needed, and Flatpak-clean.
2. **`/dev/uinput`** - the fallback when a portal isn't available.

The `uinput` fallback needs your user to have access to `/dev/uinput`, which is a one-time setup.
Verbatim detects this and shows the [E9 guided fix](/troubleshooting/#e9---verbatim-needs-permission-to-type-on-linux) with a **Set up typing** button.

The helper script (bundled in the AppImage and Flatpak) installs a udev rule like:

```
# /etc/udev/rules.d/70-verbatim-uinput.rules
KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"
```

It then adds you to the `input` group.
**Log out and back in** (or reboot) for the group change to take effect.

If your compositor supports the input portal, you can skip `uinput` entirely - Verbatim will use the portal and you'll be asked to approve it once per session (or persistently, where the compositor remembers a `restore_token`).

### GNOME versions

The GlobalShortcuts portal that drives the global hotkey needs GNOME 48 or newer.
On older GNOME, bind a custom keyboard shortcut to run `verbatim trigger toggle` instead - dictation itself works the same way.
