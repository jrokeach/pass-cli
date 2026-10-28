# pass-cli fork: alias mailboxes

Unofficial build of Proton Pass CLI plus the `feat/alias-mailboxes` branch, kept rebased on
upstream. Release `v<version>-mailbox.<n>` is upstream release `<version>` with the fork
patches applied. Not affiliated with or supported by Proton.

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
TAG=v2.5.1-mailbox.1   # latest: https://github.com/jrokeach/pass-cli/releases
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
on macOS), keyring entries and app version as the stock
release it is built on, so it should pick up an existing stock session without logging in again. This has
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

## Staying current with upstream

`main` mirrors upstream (`protonpass/pass-cli`); `fork/release` is upstream plus the fork
patches. The Upstream sync workflow (`.github/workflows/upstream-sync.yml`) checks upstream
every 6 hours, or on demand from the Actions tab:

1. It fast-forwards `main` to upstream, copies upstream's tags, rebases the fork patches onto
   upstream's tip and opens a PR from `fork/sync/<sha>` into `fork/release`.
2. Fork CI runs on that branch. When it passes, the workflow moves `fork/release` to the
   branch, which GitHub shows as the PR being merged, and pushes `v<version>-mailbox.1` for
   each new upstream release tag. Fork CI then builds and publishes those releases.
3. If the rebase conflicts, the PR is a draft labeled `upstream-sync-conflict` with the steps
   to resolve it. Push the resolved rebase to the PR branch and step 2 runs on it. Don't use
   the merge button on sync PRs.

Setup:

- Make `fork/release` the default branch (scheduled and `workflow_run` workflows only run
  from the default branch).
- Add a `FORK_SYNC_TOKEN` repository secret: a fine-grained token for this repository with
  Contents, Pull requests and Workflows read and write. The default `GITHUB_TOKEN` can't push
  the workflow files in the patches or upstream, and its pushes don't start Fork CI.
- Allow that token to force-push `fork/release` if the branch is protected.
