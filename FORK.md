# pass-cli fork: alias mailboxes

Unofficial build of Proton Pass CLI 2.5.1 (upstream commit `5d0cfbf`) plus the
`feat/alias-mailboxes` branch. Not affiliated with or supported by Proton.

Adds:

- `pass-cli item alias create --mailbox <MAILBOX>` (repeatable): choose the mailboxes a new
  alias forwards to, by mailbox email address or numeric mailbox ID.
- `pass-cli item alias update --mailbox <MAILBOX> ...`: replace an existing alias's mailboxes.
- `pass-cli item view` on an alias shows its address and mailboxes (human and `--output json`),
  and `--field mailboxes` prints them one per line.

## Install next to stock pass-cli

Install the binary under its own name and run it through a wrapper that turns off the
self-updater. `pass-cli update` downloads the official release and replaces the running
binary, which would silently swap the fork for stock pass-cli.

```bash
# Linux x86_64 (use the aarch64-apple-darwin asset on Apple silicon Macs)
TAG=v2.5.1-mailbox.1
ASSET=pass-cli-fork-$TAG-x86_64-unknown-linux-gnu
BASE=https://github.com/jrokeach/pass-cli/releases/download/$TAG

mkdir -p ~/.local/lib/pass-cli-fork ~/.local/bin
cd ~/.local/lib/pass-cli-fork
curl -fsSLO "$BASE/$ASSET" && curl -fsSLO "$BASE/$ASSET.sha256"
shasum -a 256 -c "$ASSET.sha256"          # must print OK
chmod +x "$ASSET" && ln -sf "$ASSET" pass-cli-fork-bin
# macOS only: xattr -d com.apple.quarantine "$ASSET"   (the binary is not notarized)

cat > ~/.local/bin/pass-cli-fork <<'SH'
#!/bin/sh
PROTON_PASS_NO_UPDATE_CHECK=1 exec "$HOME/.local/lib/pass-cli-fork/pass-cli-fork-bin" "$@"
SH
chmod +x ~/.local/bin/pass-cli-fork
pass-cli-fork --version
```

Never run `pass-cli-fork update`.

## Reusing an existing session

The fork has the same session code, session directory (`PROTON_PASS_SESSION_DIR`, or by default
`~/.local/share/proton-pass-cli` on Linux and `~/Library/Application Support/proton-pass-cli`
on macOS), keyring entries and app version (`2.5.1`) as stock
2.5.1, so it should pick up an existing stock session without logging in again. This has
not been verified against a real account yet. Back up the session directory first:

```bash
DIR="${PROTON_PASS_SESSION_DIR:-$HOME/.local/share/proton-pass-cli}"   # macOS: $HOME/Library/Application Support/proton-pass-cli
cp -a "$DIR" "$DIR.bak.$(date +%Y%m%d%H%M%S)"
pass-cli-fork info        # same account as `pass-cli info` means the session is shared
```

Stock pass-cli and the fork can then use the same session side by side. To give the fork its
own session, set `PROTON_PASS_SESSION_DIR` to a different directory in the wrapper and log in
there (personal access token or agent login work as usual).

## Agent sessions

`item alias update` follows the same agent rules as the other item write commands:
`PROTON_PASS_AGENT_REASON` must be set, and the update is reported as an item update.
`item alias create` keeps upstream's behavior: upstream doesn't check the agent reason for
alias creation, and this fork doesn't change that.
