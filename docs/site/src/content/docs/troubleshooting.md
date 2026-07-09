---
title: Troubleshooting
description: What every on-screen Verbatim message (E1-E10) means and how to resolve it - microphone, model, transcription, typing, and polish issues.
---

When something needs your attention, Verbatim shows a short plain-language message and, in almost every case, one button that fixes it.
This page lists each message by its code (E1-E10), what it means, and what to do.
You never lose your words: whenever Verbatim can't type, your text is placed on the clipboard so you can paste it.

## E1 - Verbatim can't hear you yet

> "Verbatim can't hear you yet - microphone access is off."

Microphone permission is off, so there's nothing to transcribe.
Click **Open microphone settings** and enable Verbatim, then try again - Verbatim re-checks the permission live.
See [Permissions -> Microphone](/permissions/#microphone-all-platforms).

## E2 - Download a speech model

> "One-time setup: download a speech model (about 150 MB)."

Verbatim needs a speech model before it can transcribe, and none is installed yet.
Click **Download model** to get the recommended model for your hardware.
This is a one-time download; afterward Verbatim works fully offline.

## E3 - Transcription hit a snag

> "Transcription hit a snag - your recording is saved."

Something went wrong while transcribing, but your recording was kept.
Click **Retry**.
If it keeps happening, try a different model in the model manager, or check that your machine isn't out of memory.

## E4 - Couldn't type into this app

> "Couldn't type into this app - your text is on the clipboard."

Verbatim transcribed you but couldn't type into the focused app, so it put the text on your clipboard.
Click **Paste anyway** (or press your paste shortcut) to drop it in.

Common causes:

- **macOS:** Accessibility permission is off - see [Permissions -> macOS](/permissions/#macos---accessibility).
- **Windows:** the target app runs as administrator (UIPI) - see [Permissions -> Windows](/permissions/#windows).
- **Linux:** neither the input portal nor `uinput` is available - see [E9](#e9---verbatim-needs-permission-to-type-on-linux).

## E5 - Password field, stayed out

> "That looks like a password field, so Verbatim stayed out. Text is on the clipboard."

This is Verbatim protecting you.
It detected a secure/password field and deliberately refused to type into it; your text is on the clipboard if you truly want to paste it.
There's no button here on purpose - decide for yourself whether to paste into a credential field.

## E6 - Your mic disconnected

> "Your mic disconnected - transcribed what I heard."

Your microphone was unplugged or switched off mid-recording.
Verbatim transcribed whatever it captured before the drop.
Click **Choose microphone** to pick another input device, then dictate again.

## E7 - The active app changed

> "The active app changed - your text is on the clipboard."

The focused app changed between when you finished speaking and when Verbatim went to type, so it couldn't be sure where the text belonged.
It's on your clipboard - click **Paste anyway** in the app you meant.

## E8 - Download interrupted

> "Download interrupted - pick up where it left off."

A model download was interrupted (network drop, quit, sleep).
In the model manager, click **Resume download** - Verbatim continues from where it stopped rather than starting over.

## E9 - Verbatim needs permission to type on Linux

> "Verbatim needs permission to type on Linux - a quick one-time setup."

On Linux, typing via the `uinput` fallback needs a one-time permission grant.
Click **Set up typing** to run the guided setup, which installs a udev rule and adds you to the `input` group.
**Log out and back in** (or reboot) afterward for it to take effect.
Full details are in [Permissions -> Linux](/permissions/#linux).

If your desktop supports the input portal (GNOME/KDE on Wayland), you can approve the portal instead and skip `uinput` entirely.

## E10 - Polish is unavailable, typed raw

> "Polish is unavailable right now, so Verbatim typed the raw text."

The polish step couldn't run (for example, the polish model isn't loaded), so rather than fail, Verbatim typed your **raw** transcript.
Your words still landed - they just weren't cleaned up.
This shows once as a tray notice, not on every dictation.
Click **Polish settings** to check your polish model, or continue in raw mode if that's what you want.

Note: when polish simply runs out of time on a slow machine, Verbatim silently types the raw text with no message at all - that's by design and isn't an error.

## Still stuck?

- Run `verbatim status` to confirm the background instance is alive.
- File an issue at [github.com/underthinker/verbatim/issues](https://github.com/underthinker/verbatim/issues) with your OS, install channel, and the E-code you saw.
