Unofficial fork build of Proton Pass CLI 2.7.0 with two features added. Not affiliated with or supported by Proton.

- **Alias mailboxes**: `item alias create --mailbox` (repeatable), `item alias update --mailbox`, mailboxes in `item view` and `--field mailboxes`.
- **Alias contacts (reverse aliases)**: `item alias contact create|list|delete|block|allow`. `create --contact <EMAIL> --output json` returns `reverse_alias_address`, the address to send to so the contact receives your email from the alias. It is idempotent (`"existed": true` for an existing contact).

Built from the `next-2.7.0` branch (upstream 2.7.0 `ff1a3c9` + `feat/alias-mailboxes-2.7.0` + `feat/alias-contacts-2.7.0`). Binaries: Linux x86_64, Linux arm64 and macOS arm64 (the macOS binary is added a little later; it is not notarized). Checksums are in `SHA256SUMS` and the per-binary `.sha256` files.

**Install under the separate name `pass-cli-fork`**, next to stock `pass-cli`, and never overwrite stock pass-cli. See [FORK.md](https://github.com/jrokeach/pass-cli/blob/next-2.7.0/FORK.md).

> **Never run `pass-cli-fork update`.** It downloads the official release and replaces the running binary with stock pass-cli, silently removing the fork features. Run the fork with `PROTON_PASS_NO_UPDATE_CHECK=1` (the wrapper in FORK.md does).

**Sessions:** this build has the same session code, session directory (`PROTON_PASS_SESSION_DIR`), keyring entries and app version (2.7.0) as stock 2.7.0; the fork features don't touch session code. A session from stock 2.7.0 should be reused as is. A session from a 2.5.1-based build (stock 2.5.1, `v2.5.1-mailbox.1`, `v2.5.1-next`) is reused exactly as stock 2.7.0 would reuse it. Not verified against a live account. Back up the session directory first (see FORK.md).
