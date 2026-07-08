# Packaging & distribution channels

Source-of-truth manifests for every distribution channel.
The `.github/workflows/channels.yml` workflow fills the version/checksum tokens and opens the channel PRs on each non-prerelease `v*` release.
Edit the templates here, never the copies pushed to the channel repos.

## Layout

| Path | Channel | Consumes |
|---|---|---|
| `homebrew/verbatim.rb` | Homebrew cask | the `.dmg` release assets (arm64 + x64) |
| `winget/*.yaml` | winget (3-file 1.6 manifest) | the `.msi` release asset |
| `flatpak/app.verbatim.dictation.{yml,metainfo.xml,desktop}` | Flathub | the `.deb` release asset |
| `linux/60-verbatim-uinput.rules` + `install-udev-rule.sh` | AppImage / Flatpak / `.deb` | shipped in-bundle, run by onboarding E9 |

The udev rule + helper are also bundled into the `.deb` and AppImage via `tauri.conf.json` (`bundle.linux.*.files`), installed to `/usr/share/verbatim/`.

## Enabling a channel

Each channel job in `channels.yml` is dormant until its **secret** is set; absent secret = a loud `::warning::` and a no-op (fork/PR safe, same degrade-loud contract as the signing steps in `release.yml`).
Target repos are overridable **variables** with sensible defaults.

| Channel | Secret (token) | Variable (target repo, default) |
|---|---|---|
| Homebrew | `HOMEBREW_TAP_TOKEN` | `HOMEBREW_TAP_REPO` (`underthinker/homebrew-verbatim`) |
| winget | `WINGET_TOKEN` | `WINGET_FORK_REPO` (`underthinker/winget-pkgs`), `WINGET_UPSTREAM_REPO` (`microsoft/winget-pkgs`) |
| Flathub | `FLATHUB_TOKEN` | `FLATHUB_REPO` (`flathub/app.verbatim.dictation`) |

Tokens are PATs with `repo` scope on the respective target.
Create the tap / winget fork / Flathub app repo first, then set the secret.

## Version invariant

The manifests assume release asset filenames carry the tag's version (`Verbatim_<version>_*`), i.e. `tauri.conf.json` `version` == the `v*` tag.
If a version-matched asset is missing from the release, `channels.yml` warns and skips that channel rather than publishing a manifest that 404s.
Keeping the bundle version in lockstep with the tag is a release-config follow-up (tracked from Phase A).

## Not yet ticked

Acceptance criterion 2 (clean-machine installs from each channel) needs real signed artifacts and the live channel repos; it is recorded during the Phase E dogfood, not here.
This phase lands the manifests + guarded automation only.
