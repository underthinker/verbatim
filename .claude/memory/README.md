# Claude memory (repo-synced)

Claude Code's persistent per-project memory for verbatim, checked into the repo so it
syncs across machines. Claude reads/writes it from a path under `~/.claude/`, not here,
so each machine needs a one-time symlink pointing that path at this directory.

## Setup on a new machine

After cloning, from the repo root:

```sh
# slug is the repo's absolute path with '/' -> '-'  (e.g. /home/titon/verbatim -> -home-titon-verbatim)
SLUG=$(pwd | tr '/' '-')
LINK="$HOME/.claude/projects/$SLUG/memory"
mkdir -p "$(dirname "$LINK")"
rm -rf "$LINK"                       # remove any local-only memory dir first
ln -s "$PWD/.claude/memory" "$LINK"
```

Verify: `head -1 "$LINK/MEMORY.md"` should print `# Verbatim memory index`.

## Notes

- `MEMORY.md` is the index loaded each session; the other `*.md` files are one fact each.
- The symlink lives outside the repo, so it is never committed — re-run the setup per machine.
- If the repo lives at a different absolute path on another machine, `SLUG` differs
  automatically; the command above handles it.
