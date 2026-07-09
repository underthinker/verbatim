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

## Building

```sh
cd docs/site
pnpm install
pnpm dev      # preview at localhost:4321/verbatim
pnpm build    # static output in dist/
```

`.github/workflows/docs.yml` builds the site on every PR touching `docs/site/` and publishes `main` to GitHub Pages at <https://underthinker.github.io/verbatim>.
