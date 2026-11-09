# pass-cli fork: alias mailboxes + alias contacts (`next-2.7.0`)

Unofficial build of Proton Pass CLI with two features added. Not affiliated with or supported
by Proton. Releases `v<upstream>-next.<n>` are built from an integration branch only and
always carry both features: `v2.7.0-next.<n>` from `next-2.7.0` (this branch, upstream
2.7.0), `v2.5.1-next.<n>` from `next` (upstream 2.5.1).

Adds:

- **Alias mailboxes** (`feat/alias-mailboxes-2.7.0`)
  - `pass-cli item alias create --mailbox <MAILBOX>` (repeatable): choose the mailboxes a new
    alias forwards to, by mailbox email address or numeric mailbox ID.
  - `pass-cli item alias update --mailbox <MAILBOX> ...`: replace an existing alias's mailboxes.
  - `pass-cli item view` on an alias shows its address and mailboxes (human and `--output json`),
    and `--field mailboxes` prints them one per line.
- **Alias contacts / reverse aliases** (`feat/alias-contacts-2.7.0`). A contact is a recipient
  attached to one alias. Proton generates a reverse alias for it: an address you send to, which
  delivers your email to the contact as if it came from the alias.
  - `pass-cli item alias contact create (--item-title <T> | --item-id <I>) --contact <EMAIL>
    [--name <NAME>] [--output json]`: prints the reverse alias. It is idempotent: an existing
    contact for the address is returned with `"existed": true`. JSON has `contact`,
    `reverse_alias_address` (the address to send to), `contact_id`, `blocked`, `existed`, plus
    `reverse_alias` (`Name <address>`) only if the API returns that form (it currently doesn't).
  - `pass-cli item alias contact list (--item-title <T> | --item-id <I>) [--output json]`
  - `pass-cli item alias contact delete|block|allow ... (--contact <EMAIL> | --contact-id <ID>)`

All of them take `--vault-name <V>` or `--share-id <S>` (or the default vault). Reference:
`docs/public/docs/commands/item.md`, sections `alias create`, `alias update`, `alias contact`.

## Install next to stock pass-cli

Install the binary under its own name and run it through a wrapper that turns off the
self-updater.

**Never run `pass-cli-fork update`.** `update` downloads the official release and replaces
the running binary, which would silently swap the fork for stock pass-cli.

```bash
# Linux x86_64. Linux arm64: aarch64-unknown-linux-gnu. Apple silicon Macs: aarch64-apple-darwin
TAG=v2.7.0-next.1   # latest: https://github.com/jrokeach/pass-cli/releases
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

Upgrading from `v2.5.1-mailbox.1` or a `v2.5.1-next` release means installing the new asset
and re-pointing the `pass-cli-fork-bin` symlink; the wrapper stays the same.

## Reusing an existing session

This build has the same session code, session directory (`PROTON_PASS_SESSION_DIR`, or by
default `~/.local/share/proton-pass-cli` on Linux and `~/Library/Application Support/proton-pass-cli`
on macOS), keyring entries and app version as stock 2.7.0. The fork features don't touch
session code. A session from stock 2.7.0 should be picked up as is. A session from a 2.5.1-based
build (stock 2.5.1, `v2.5.1-mailbox.1`, `v2.5.1-next`) is reused the same way stock 2.7.0
reuses a 2.5.1 session, i.e. it depends on upstream's 2.5.1 → 2.7.0 compatibility, not on the
fork. None of this has been verified against a real account yet. Back up the session directory first:

```bash
DIR="${PROTON_PASS_SESSION_DIR:-$HOME/.local/share/proton-pass-cli}"   # macOS: $HOME/Library/Application Support/proton-pass-cli
cp -a "$DIR" "$DIR.bak.$(date +%Y%m%d%H%M%S)"
pass-cli-fork info        # same account as `pass-cli info` means the session is shared
```

To give the fork its own session, set `PROTON_PASS_SESSION_DIR` to a different directory in the
wrapper and log in there (personal access token or agent login work as usual).

## Agent sessions

- `item alias update` follows the same agent rules as the other item write commands:
  `PROTON_PASS_AGENT_REASON` must be set, and the update is reported as an item update.
  `item alias create` keeps upstream's behavior (no agent reason check).
- `item alias contact` requires `PROTON_PASS_AGENT_REASON` for every subcommand. `list` (and
  `create` of an existing contact) is reported as an item read; `create`, `delete`, `block`
  and `allow` as item updates. If the server refuses an agent or personal access token session
  (HTTP 403, or code 2011/2026/2027), the error says so, with the status, code and server message.
- Whether the server allows the alias contact endpoints for agent sessions has not been
  verified yet (see the gate check below).

Alias contact endpoints, keyed by the alias item's share ID and item ID (no separate alias ID):
`GET|POST /pass/v1/share/{shareId}/alias/{itemId}/contact`,
`DELETE /pass/v1/share/{shareId}/alias/{itemId}/contact/{contactId}`,
`PUT /pass/v1/share/{shareId}/alias/{itemId}/contact/{contactId}/blocked`.

## Branches

| Branch | Content |
| --- | --- |
| `main` | Mirror of upstream `protonpass/pass-cli` (at 2.7.0, `ff1a3c9`) |
| `feat/alias-mailboxes-2.7.0` | Upstream 2.7.0 (`ff1a3c9`) + the mailbox feature. Nothing else. |
| `feat/alias-contacts-2.7.0` | Upstream 2.7.0 (`ff1a3c9`) + the contacts feature (+ a contacts-only `FORK.md`). Nothing else. |
| `next-2.7.0` | Upstream 2.7.0, both 2.7.0 feature branches merged in, plus fork-only commits (this file, CI, the `Cargo.toml` workspace fix). `v2.7.0-next.<n>` releases are built from here. |
| `feat/alias-mailboxes`, `feat/alias-contacts`, `next` | The same three branches on upstream 2.5.1 (`5d0cfbf`), kept in place. `v2.5.1-next.<n>` releases come from `next`. |
| `fork/release` | The earlier mailbox-only release line (`v2.5.1-mailbox.1`). |

The feature branches are developed in parallel, each from the same upstream commit, and never
contain each other's code. Conflicts between them are resolved in the integration branch only.
Each upstream base has its own set of three branches; a fix to a feature goes to that
feature's branch for every base still maintained.

## Re-merging into `next-2.7.0`

After a feature branch changes:

```bash
git fetch origin
git checkout next-2.7.0 && git pull --ff-only origin next-2.7.0
git merge --no-ff origin/feat/alias-mailboxes-2.7.0
git merge --no-ff origin/feat/alias-contacts-2.7.0
```

Resolve conflicts in the merge commit on the integration branch; never fix one feature by
editing the other feature's branch. Known conflict points (the same on both bases):

- `pass/src/lib.rs`: both features add a `pub use item::alias::...` line. Keep both.
- `pass-cli/src/commands/item/alias/mod.rs`: both add `AliasCommands` variants and match arms.
  Check the result even when git reports no conflict: on both bases git put the
  `AliasCommands::Contact` arm inside the mailbox tests module instead of `run()`.
- `docs/public/docs/commands/item.md`: keep `alias create`, `alias update`, `alias contact` in
  that order.
- `FORK.md`: the contacts branch has its own contacts-only `FORK.md`. Keep the integration
  branch's version (`git checkout --ours FORK.md`) and copy over anything new by hand.

Then check that each feature is unchanged and run the checks:

```bash
git diff origin/feat/alias-mailboxes-2.7.0 HEAD --stat   # only contacts files, merge points, fork-only files
git diff origin/feat/alias-contacts-2.7.0 HEAD --stat    # only mailbox files, merge points, fork-only files
cargo clippy -p pass -p pass-cli --all-targets -- -D warnings
cargo test -p pass && cargo test -p pass-cli
git push origin next-2.7.0
```

`Next CI` (`.github/workflows/next-ci.yml`) runs the same checks and builds the three binaries
on every push to `next` or `next-*`.

## Moving to a newer upstream release

This is how 2.7.0 was set up, keeping the 2.5.1 branches in place:

1. `main` = the upstream tag (`ff1a3c9` for 2.7.0).
2. For each feature, `git checkout -b feat/<feature>-<new> origin/feat/<feature>-<old>` and
   `git rebase --onto <new upstream tag> <old upstream tag>`, resolving conflicts in the feature
   commit itself (2.7.0: upstream's new `pub mod bulk;` next to `pub(crate) mod alias;` in
   `pass/src/item/mod.rs`, and upstream's new `item move` docs section after the mailbox
   paragraph in `item.md`). Build and test each branch on its own.
3. `next-<new>` = the upstream tag, then merge both new feature branches, then cherry-pick the
   fork-only commit from the previous integration branch and update FORK.md, the release
   notes and the CI trigger.
4. Start a new tag series `v<new upstream>-next.1`.

Note: the Upstream sync workflow on `fork/release` (from the earlier mailbox-only model)
force-moves `feat/alias-mailboxes` onto the newest upstream when it succeeds, which would move
that 2.5.1 branch. Disable that workflow to keep the 2.5.1 branches in place.

## Releasing

1. Update `.github/next-release-notes.md` (the release body) on the integration branch if
   needed, and push.
2. Tag the tested integration commit and push the tag:

   ```bash
   git tag v2.7.0-next.N next-2.7.0 && git push origin v2.7.0-next.N
   ```

3. `Next CI` publishes a prerelease with the Linux x86_64 and arm64 binaries and `SHA256SUMS`,
   then adds the macOS arm64 binary and updates `SHA256SUMS` when that build finishes.

Tags count up per upstream base: `v<upstream version>-next.<n>`.

## Agent-session gate check

The contacts feature is only useful if a scoped agent session may call the contact endpoints.
Before the first release, run this with the agent session (Editor on one vault) and a
throwaway alias in that vault. Use the binary of the latest `Next CI` run on `next-2.7.0` (Actions →
Next CI → run → Artifacts, e.g. `x86_64-unknown-linux-gnu`), or build it with
`cargo build --release -p pass-cli`, and install it as `pass-cli-fork` as above:

```bash
export PROTON_PASS_AGENT_REASON="Testing alias contacts"
pass-cli-fork item alias create --vault-name "<V>" --prefix "contact-test" --output json
pass-cli-fork item alias contact create --vault-name "<V>" --item-title "Alias for contact-test" \
  --contact recipient@example.com --output json
```

- It prints `reverse_alias_address`: the gate passes.
- It fails with "The Proton API refused to let an agent session manage alias contacts (HTTP 403,
  code ...)": the server refuses agent sessions. Don't release; report the error upstream.
