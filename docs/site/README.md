# Verbatim end-user documentation

The end-user docs site, built with [Astro Starlight](https://starlight.astro.build/).
Content lives in `src/content/docs/` as plain Markdown, so it is readable directly on GitHub without building.

## Pages

- `index.md` - what Verbatim is
- `install.md` - install per channel, per OS
- `permissions.md` - the microphone / Accessibility / typing grants
- `using.md` - hotkeys, dictionary, profiles, raw mode, CLI
- `troubleshooting.md` - the E1-E10 message catalog

The troubleshooting copy is kept in sync with the app's error catalog (`crates/verbatim-app/src/error_catalog.rs`); when a message changes there, update it here too.

## Building (follow-up)

The Starlight config (`astro.config.mjs`, `package.json`) is in place; wiring the build into CI and publishing to GitHub Pages is a follow-up.
To preview locally once the toolchain is set up:

```sh
cd docs/site
npm install
npm run dev
```
