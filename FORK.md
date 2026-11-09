# pass-cli fork: alias contacts (`feat/alias-contacts-2.7.0`)

This branch adds alias contacts (reverse aliases) to Proton Pass CLI. It is upstream release
2.7.0 (`ff1a3c9`) plus this one feature, so it can be offered upstream on its own. The same
feature on upstream 2.5.1 (`5d0cfbf`) stays on `feat/alias-contacts`. Not
affiliated with or supported by Proton.

Fork binaries are not built from this branch: the released fork binary is built from
`next-2.7.0`, which combines this branch with `feat/alias-mailboxes-2.7.0`. See `FORK.md` on
`next-2.7.0` for install and release notes.

## What it adds

A contact is a recipient attached to one alias. Creating it makes Proton generate a reverse
alias: an address you send to, which delivers the email to the contact as if it came from the
alias, so your real mailbox stays hidden.

```bash
pass-cli item alias contact create --vault-name <V> (--item-title <T> | --item-id <I>) \
  --contact <EMAIL> [--name <NAME>] [--output json]
pass-cli item alias contact list   --vault-name <V> (--item-title <T> | --item-id <I>) [--output json]
pass-cli item alias contact delete --vault-name <V> (--item-title <T> | --item-id <I>) \
  (--contact <EMAIL> | --contact-id <ID>) [--output json]
pass-cli item alias contact block  ... (--contact <EMAIL> | --contact-id <ID>)
pass-cli item alias contact allow  ... (--contact <EMAIL> | --contact-id <ID>)
```

`--share-id` can replace `--vault-name`, and neither is needed with a default vault.

- `create` checks the address before any request. It is idempotent: if the alias already has a
  contact for the address, it returns that contact with `"existed": true` and creates nothing.
- JSON output (`create`): `item_id`, `alias`, `contact_id`, `contact`, `name`,
  `reverse_alias_address` (the address to send to), `blocked`, `create_time`, `existed`.
  `reverse_alias` (`Name <address>`) is added only if the API returns that form; it currently
  returns a bare address. `list` returns `{item_id, alias, contacts: [...]}`.

Full reference: `docs/public/docs/commands/item.md`, section `alias contact`.

## API

The endpoints are the ones the Proton Pass web app uses (`packages/pass/lib/alias/alias.requests.ts`
in ProtonMail/WebClients):

| Command | Request |
| --- | --- |
| `list` | `GET /pass/v1/share/{shareId}/alias/{itemId}/contact?Since={LastID}`, repeated until `LastID` is 0 |
| `create` | `POST /pass/v1/share/{shareId}/alias/{itemId}/contact` with `{"Email": ..., "Name": ...}` |
| `delete` | `DELETE /pass/v1/share/{shareId}/alias/{itemId}/contact/{contactId}` |
| `block` / `allow` | `PUT /pass/v1/share/{shareId}/alias/{itemId}/contact/{contactId}/blocked` with `{"Blocked": true/false}` |

Contacts are addressed by the alias item's share ID and item ID, the same as the other
`/pass/v1/share/{shareId}/alias/{itemId}` endpoints. There is no separate numeric alias ID
(as in SimpleLogin's `/api/aliases/:alias_id/contacts`) to look up. Contact IDs are numeric. The
contact object is `{ID, Name, Email, ReverseAlias, Blocked, CreateTime}`, and the list adds
`ForwardedEmails`, `RepliedEmails` and `BlockedEmails` (last 14 days). The API has no
"existed" flag, so the CLI lists the contacts before creating one.

## Agent sessions

`item alias contact` follows the rules of the other item commands: `PROTON_PASS_AGENT_REASON`
must be set. `list`, and `create` of an existing contact, are reported as item reads; the
other changes as item updates. If the API refuses an agent or personal access token session
(HTTP 403, or code 2011/2026/2027), the error says so and includes the status, code and
server message.

Whether the server allows these endpoints for agent sessions has not been verified yet.
