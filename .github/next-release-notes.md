Unofficial fork build of Proton Pass CLI 2.5.1 with two features added. Not affiliated with or supported by Proton.

- **Alias mailboxes**: `item alias create --mailbox` (repeatable), `item alias update --mailbox`, mailboxes in `item view` and `--field mailboxes`.
- **Alias contacts (reverse aliases)**: `item alias contact create|list|delete|block|allow`. `create --contact <EMAIL> --output json` returns `reverse_alias_address`, the address to send to so the contact receives your email from the alias. It is idempotent (`"existed": true` for an existing contact).

Built from the `next` branch (upstream 2.5.1 `5d0cfbf` + `feat/alias-mailboxes` + `feat/alias-contacts`). Binaries: Linux x86_64, Linux arm64 and macOS arm64 (the macOS binary is added a little later; it is not notarized). Checksums are in `SHA256SUMS` and the per-binary `.sha256` files.

**Install under the separate name `pass-cli-fork`**, next to stock `pass-cli`, and never overwrite stock pass-cli. See [FORK.md](https://github.com/jrokeach/pass-cli/blob/next/FORK.md).

> **Never run `pass-cli-fork update`.** It downloads the official release and replaces the running binary with stock pass-cli, silently removing the fork features. Run the fork with `PROTON_PASS_NO_UPDATE_CHECK=1` (the wrapper in FORK.md does).

**Sessions:** this build has the same session code, session directory (`PROTON_PASS_SESSION_DIR`), keyring entries and app version (2.5.1) as stock 2.5.1 and `v2.5.1-mailbox.1`. Compared with `v2.5.1-mailbox.1`, it changes only the alias contacts code. It is therefore expected to reuse an existing session from either, without logging in again. This has not been verified against a live account. Back up the session directory first (see FORK.md).
