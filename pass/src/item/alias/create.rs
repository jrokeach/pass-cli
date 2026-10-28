/*
 *  Copyright (c) 2026 Proton AG
 *  This file is part of Proton AG and Proton Pass.
 *
 *  Proton Pass is free software: you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation, either version 3 of the License, or
 *  (at your option) any later version.
 *
 *  Proton Pass is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with Proton Pass.  If not, see <https://www.gnu.org/licenses/>.
 *
 */

use super::options::{AliasMailbox, AliasOptionsResponse};
use crate::domain::{AliasItem, ItemContent, ItemId, ItemType, ShareId};
use crate::item::create::ItemCreatedEvent;
use crate::item::create::common::{CreateItemRequest, CreateItemResponse};
use crate::permission::PermissionAction;
use crate::{PassClient, PassClientContext};
use anyhow::{Context, Result, anyhow};
use muon::POST;
use pass_derive::sdk_export;

#[sdk_export]
#[derive(Debug)]
pub struct CreatedAliasItem {
    pub alias: String,
    pub item_id: ItemId,
}

#[derive(serde::Serialize)]
struct CreateAliasRequest {
    #[serde(rename = "Prefix")]
    pub prefix: String,
    #[serde(rename = "SignedSuffix")]
    pub signed_suffix: String,
    #[serde(rename = "MailboxIDs")]
    pub mailbox_ids: Vec<i64>,
    #[serde(rename = "AliasName")]
    pub alias_name: Option<String>,
    #[serde(rename = "Item")]
    pub item: CreateItemRequest,
}

#[sdk_export]
impl<C: PassClientContext> PassClient<C> {
    #[sdk_export]
    pub async fn create_alias(&self, share_id: &ShareId, prefix: &str) -> Result<CreatedAliasItem> {
        self.create_alias_with_mailboxes(share_id, prefix, &[])
            .await
    }
}

impl<C: PassClientContext> PassClient<C> {
    /// Creates an alias that forwards to the given mailboxes. Each entry can be a mailbox email
    /// address or a numeric mailbox ID, as listed in the alias options of the share. An empty
    /// list uses the first mailbox the alias options return, the same as [`Self::create_alias`].
    pub async fn create_alias_with_mailboxes(
        &self,
        share_id: &ShareId,
        prefix: &str,
        mailboxes: &[String],
    ) -> Result<CreatedAliasItem> {
        self.action_guard(PermissionAction::CreateAlias {
            share_id: share_id.clone(),
        })
        .await?;
        let request = self
            .create_alias_request(share_id, prefix, mailboxes)
            .await
            .context("Error creating create_alias request")?;

        let req = POST!("/pass/v1/share/{share_id}/alias/custom")
            .body_json(request)
            .context("Error serializing create_alias request")?;
        let res = self
            .send(req)
            .await
            .context("Error sending create alias request")?;
        let response: CreateItemResponse = assert_response!(res);

        self.clear_items_cache(share_id).await;

        let email = match response.item.alias_email {
            Some(email) => email,
            None => return Err(anyhow!("Error getting email from created alias")),
        };

        let item_id = ItemId::new(response.item.item_id);

        self.emit_telemetry(&ItemCreatedEvent {
            item_type: ItemType::Alias,
        })
        .await;

        Ok(CreatedAliasItem {
            alias: email,
            item_id,
        })
    }

    async fn create_alias_request(
        &self,
        share_id: &ShareId,
        prefix: &str,
        mailboxes: &[String],
    ) -> Result<CreateAliasRequest> {
        let options = self
            .get_alias_options(share_id)
            .await
            .context("Error fetching alias options")?;

        // Resolve the mailboxes before building the item, so an unknown mailbox fails early
        let mailbox_ids = resolve_create_mailbox_ids(&options, mailboxes)?;

        let title = format!("Alias for {prefix}");
        let item = self
            .create_item_request(share_id, &title, "", ItemContent::Alias(AliasItem), None)
            .await
            .context("Error creating create_alias_item request")?;

        build_create_alias_request(&options, prefix, mailbox_ids, item)
    }
}

/// Mailbox IDs for a new alias: the requested ones, or the first available mailbox when none
/// were requested (the behavior before mailboxes could be chosen).
fn resolve_create_mailbox_ids(
    options: &AliasOptionsResponse,
    requested: &[String],
) -> Result<Vec<i64>> {
    if requested.is_empty() {
        let mailbox = options.mailboxes.first().context("No mailbox found")?;
        return Ok(vec![mailbox.id]);
    }
    resolve_mailbox_ids(&options.mailboxes, requested)
}

fn build_create_alias_request(
    options: &AliasOptionsResponse,
    prefix: &str,
    mailbox_ids: Vec<i64>,
    item: CreateItemRequest,
) -> Result<CreateAliasRequest> {
    let suffix = options.suffixes.first().context("No suffix found")?;
    Ok(CreateAliasRequest {
        prefix: prefix.to_string(),
        signed_suffix: suffix.signed_suffix.to_string(),
        mailbox_ids,
        alias_name: None,
        item,
    })
}

/// Resolves each requested mailbox (an email address, case-insensitive, or a numeric mailbox ID)
/// against `available`. Duplicates are dropped, keeping the first occurrence. Fails, listing the
/// valid mailboxes, if any requested mailbox is not available or nothing was requested.
pub(crate) fn resolve_mailbox_ids(
    available: &[AliasMailbox],
    requested: &[String],
) -> Result<Vec<i64>> {
    if requested.is_empty() {
        return Err(anyhow!("At least one mailbox is required"));
    }

    let mut ids = Vec::with_capacity(requested.len());
    let mut unknown = Vec::new();
    for value in requested {
        let value = value.trim();
        let found = available.iter().find(|mailbox| {
            mailbox.email.eq_ignore_ascii_case(value)
                || value.parse::<i64>().is_ok_and(|id| id == mailbox.id)
        });
        match found {
            Some(mailbox) => {
                if !ids.contains(&mailbox.id) {
                    ids.push(mailbox.id);
                }
            }
            None => unknown.push(value.to_string()),
        }
    }

    if !unknown.is_empty() {
        let valid = if available.is_empty() {
            "none".to_string()
        } else {
            available
                .iter()
                .map(|mailbox| format!("{} (ID {})", mailbox.email, mailbox.id))
                .collect::<Vec<_>>()
                .join(", ")
        };
        return Err(anyhow!(
            "Unknown mailbox: {}. Valid mailboxes: {valid}",
            unknown.join(", ")
        ));
    }

    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::alias::options::AliasSuffix;
    use crate::item::list::ItemRevision;
    use crate::test_tools::*;

    const OWNER: &str = "owner@example.com";
    const SECOND: &str = "second-inbox@example.com";

    fn mailboxes() -> Vec<AliasMailbox> {
        vec![
            AliasMailbox {
                id: 11,
                email: OWNER.to_string(),
            },
            AliasMailbox {
                id: 22,
                email: SECOND.to_string(),
            },
        ]
    }

    fn options() -> AliasOptionsResponse {
        AliasOptionsResponse {
            suffixes: vec![AliasSuffix {
                suffix: ".abc@example.net".to_string(),
                signed_suffix: ".abc@example.net.signature".to_string(),
            }],
            mailboxes: mailboxes(),
            can_create_alias: true,
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn item_request() -> CreateItemRequest {
        CreateItemRequest {
            key_rotation: 1,
            content_format_version: 1,
            content: "content".to_string(),
            item_key: "key".to_string(),
            folder_id: None,
        }
    }

    #[test]
    fn resolves_mailbox_by_email_case_insensitively() {
        let ids = resolve_mailbox_ids(&mailboxes(), &strings(&["Second-Inbox@Example.com"]))
            .expect("should resolve");
        assert_eq!(vec![22], ids);
    }

    #[test]
    fn resolves_mailbox_by_id() {
        let ids = resolve_mailbox_ids(&mailboxes(), &strings(&["22", "11"])).expect("resolve");
        assert_eq!(vec![22, 11], ids);
    }

    #[test]
    fn resolves_multiple_mailboxes_in_order_without_duplicates() {
        let ids = resolve_mailbox_ids(&mailboxes(), &strings(&[OWNER, SECOND, "11"]))
            .expect("should resolve");
        assert_eq!(vec![11, 22], ids);
    }

    #[test]
    fn unknown_mailbox_lists_valid_mailboxes() {
        let err = resolve_mailbox_ids(&mailboxes(), &strings(&[OWNER, "nobody@example.com", "99"]))
            .expect_err("unknown mailbox must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("Unknown mailbox: nobody@example.com, 99"),
            "{msg}"
        );
        assert!(msg.contains("owner@example.com (ID 11)"), "{msg}");
        assert!(msg.contains("second-inbox@example.com (ID 22)"), "{msg}");
    }

    #[test]
    fn empty_request_is_rejected_when_mailboxes_are_required() {
        assert!(resolve_mailbox_ids(&mailboxes(), &[]).is_err());
    }

    #[test]
    fn create_without_mailboxes_uses_first_mailbox() {
        let ids = resolve_create_mailbox_ids(&options(), &[]).expect("should resolve");
        assert_eq!(vec![11], ids);
    }

    #[test]
    fn create_request_without_mailboxes_matches_previous_behavior() {
        let ids = resolve_create_mailbox_ids(&options(), &[]).unwrap();
        let req = build_create_alias_request(&options(), "shop", ids, item_request()).unwrap();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!("shop", json["Prefix"]);
        assert_eq!(".abc@example.net.signature", json["SignedSuffix"]);
        assert_eq!(serde_json::json!([11]), json["MailboxIDs"]);
        assert_eq!(serde_json::Value::Null, json["AliasName"]);
    }

    #[test]
    fn create_request_with_mailboxes_carries_all_ids() {
        let ids = resolve_create_mailbox_ids(&options(), &strings(&[OWNER, SECOND])).unwrap();
        let req = build_create_alias_request(&options(), "shop", ids, item_request()).unwrap();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(serde_json::json!([11, 22]), json["MailboxIDs"]);
    }

    const SHARE_ID: &str = "MyShareID";
    const ITEM_ID: &str = "MyItemID";
    const ALIAS: &str = "shop.abc@example.net";

    fn setup_alias_endpoints(api: &ProtonAPI) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        api.handler_with_method(
            Method::GET,
            format!("/pass/v1/share/{SHARE_ID}/alias/options"),
            move |_| {
                success(serde_json::json!({
                    "Options": {
                        "Suffixes": [{
                            "Suffix": ".abc@example.net",
                            "SignedSuffix": ".abc@example.net.signature",
                            "IsCustom": false,
                            "IsPremium": false,
                            "Domain": "example.net"
                        }],
                        "Mailboxes": [
                            {"ID": 11, "Email": OWNER},
                            {"ID": 22, "Email": SECOND}
                        ],
                        "CanCreateAlias": true
                    }
                }))
            },
        );
        api.handler_with_method(
            Method::POST,
            format!("/pass/v1/share/{SHARE_ID}/alias/custom"),
            move |_| {
                success(CreateItemResponse {
                    item: ItemRevision {
                        item_id: ITEM_ID.to_string(),
                        revision: 1,
                        content_format_version: 1,
                        key_rotation: 1,
                        content: "".to_string(),
                        item_key: None,
                        state: 1,
                        flags: 0,
                        alias_email: Some(ALIAS.to_string()),
                        create_time: 0,
                        modify_time: 0,
                        folder_id: None,
                    },
                })
            },
        )
    }

    #[muon_test::test]
    async fn test_create_alias_default_mailbox(server: muon_test::Server) {
        let (raw_client, api) = server.client::<()>();
        let client = make_test_pass_client_with_setup(raw_client, &api, PlanType::Plus).await;
        setup_share_keys(&api, SHARE_ID);
        setup_vault_share(&api, SHARE_ID);
        let created = setup_alias_endpoints(&api);
        let recorder = api.new_recorder();

        let res = client
            .create_alias(&share_id!(SHARE_ID), "shop")
            .await
            .expect("Should create alias");

        assert_hit!(created);
        assert_eq!(ALIAS, res.alias);
        assert_eq!(ITEM_ID, res.item_id.value());
        let request: serde_json::Value = last_request!(recorder);
        assert_eq!(serde_json::json!([11]), request["MailboxIDs"]);
    }

    #[muon_test::test]
    async fn test_create_alias_with_mailboxes(server: muon_test::Server) {
        let (raw_client, api) = server.client::<()>();
        let client = make_test_pass_client_with_setup(raw_client, &api, PlanType::Plus).await;
        setup_share_keys(&api, SHARE_ID);
        setup_vault_share(&api, SHARE_ID);
        let created = setup_alias_endpoints(&api);
        let recorder = api.new_recorder();

        client
            .create_alias_with_mailboxes(&share_id!(SHARE_ID), "shop", &strings(&[OWNER, "22"]))
            .await
            .expect("Should create alias");

        assert_hit!(created);
        let request: serde_json::Value = last_request!(recorder);
        assert_eq!(serde_json::json!([11, 22]), request["MailboxIDs"]);
        assert_eq!("shop", request["Prefix"]);
    }

    #[muon_test::test]
    async fn test_create_alias_unknown_mailbox_does_not_create(server: muon_test::Server) {
        let (raw_client, api) = server.client::<()>();
        let client = make_test_pass_client_with_setup(raw_client, &api, PlanType::Plus).await;
        setup_share_keys(&api, SHARE_ID);
        setup_vault_share(&api, SHARE_ID);
        let created = setup_alias_endpoints(&api);

        let err = client
            .create_alias_with_mailboxes(
                &share_id!(SHARE_ID),
                "shop",
                &strings(&["nobody@example.com"]),
            )
            .await
            .expect_err("Unknown mailbox must fail");

        assert_not_hit!(created);
        assert!(format!("{err:#}").contains("Valid mailboxes"), "{err:#}");
    }
}
