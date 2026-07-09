---
title: Using Verbatim
description: Hotkeys and push-to-talk vs toggle, the personal dictionary, per-app profiles, raw mode, history, and the command line.
---

## The dictation hotkey

By default:

- **macOS:** hold the **right Option** key and speak (push-to-talk). Release to finish.
- **Windows and Linux:** press **Ctrl + Alt + Space** to start, and again to stop (toggle).

You can change the key and the mode in **Settings -> Hotkey**.

### Push-to-talk vs toggle

- **Hold (push-to-talk):** recording lasts exactly as long as you hold the key. Best for short, frequent dictations.
- **Toggle:** one press starts, the next press stops. Best for longer passages where holding a key is awkward.

If you prefer to set these from the environment (for scripting or a custom launcher):

```sh
VERBATIM_HOTKEY="CTRL+ALT+SPACE"   # or a bare modifier like "RightOption"
VERBATIM_HOTKEY_MODE="toggle"      # or "hold"
```

## Raw mode - skip the polish

Sometimes you want exactly what you said, with no cleanup - code, commands, or verbatim quotes.
Turn polish off globally in **Settings -> Polish**, or hold the **raw-mode modifier** (Shift by default) as you start a dictation to bypass polish for that one utterance.
When polish is skipped, Verbatim types the raw transcript straight through.

## Personal dictionary

Names, jargon, and product spellings that a general model gets wrong can be taught once.
Add them in **Settings -> Dictionary**.
Entries are applied as a deterministic post-pass after transcription, so "verbatim" always comes out capitalized the way you want and your teammate "Sayeed" stops becoming "Saeed".

## Per-app profiles

Verbatim can polish differently depending on which app you're dictating into.
A chat app might want casual, lightly-punctuated text; your editor might want none of that.
Configure per-app behavior in **Settings -> Profiles**; Verbatim matches on the focused app and applies the matching profile automatically.

## History

Every dictation is saved locally as a raw/polished pair with the target app and a timestamp, so you can recover something you lost or copy it again.
Browse it in the **History** tab.
History never leaves your machine.
You control how long it's kept in **Settings -> History** (retention in days; set it to `0` to disable history entirely), and **Clear history** wipes it immediately.

## The overlay

A small, non-focusing overlay shows Verbatim's current state - arming, listening, transcribing, polishing, done.
It never steals focus from the app you're typing into, and it respects your system's reduced-motion setting.

## Command line

Verbatim ships a `verbatim` CLI, useful for scripting and for binding your own shortcut:

| Command | What it does |
|---|---|
| `verbatim gui` | Run the desktop app (the normal way to use Verbatim). |
| `verbatim daemon` | Run the background instance only (no window). |
| `verbatim trigger start\|stop\|toggle` | Drive dictation from a script or a custom keyboard shortcut. |
| `verbatim status` | Print the current session state. |
| `verbatim stats` | Print local session/crash counters (for a dogfood report). Nothing is sent anywhere. |

On GNOME older than 48, bind a custom keyboard shortcut to `verbatim trigger toggle` to get a global hotkey.
