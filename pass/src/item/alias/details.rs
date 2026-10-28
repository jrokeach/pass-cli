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

use super::create::resolve_mailbox_ids;
use super::options::AliasMailbox;
use crate::domain::{ItemId, ShareId};
use crate::permission::PermissionAction;
use crate::{PassClient, PassClientContext};
use anyhow::{Context, Result};
use muon::{GET, POST};

/// Forwarding information of an existing alias.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct AliasDetails {
    #[serde(rename(deserialize = "Email"))]
    pub email: String,
    /// Mailboxes that receive the emails sent to this alias
    #[serde(rename(deserialize = "Mailboxes"))]
    pub mailboxes: Vec<AliasMailbox>,
    /// Mailboxes that can be linked to this alias
    #[serde(rename(deserialize = "AvailableMailboxes"), default)]
    pub available_mailboxes: Vec<AliasMailbox>,
}

#[derive(serde::Deserialize)]
struct GetAliasDetailsResponse {
    #[serde(rename = "Alias")]
    alias: AliasDetails,
}

#[derive(serde::Serialize)]
struct SetAliasMailboxesRequest {
    #[serde(rename = "MailboxIDs")]
    mailbox_ids: Vec<i64>,
}

impl<C: PassClientContext> PassClient<C> {
    /// Fetches the forwarding information (alias email and mailboxes) of an alias item.
    pub async fn get_alias_details(
        &self,
        share_id: &ShareId,
        item_id: &ItemId,
    ) -> Result<AliasDetails> {
        let res = self
            .send(GET!("/pass/v1/share/{share_id}/alias/{item_id}"))
            .await
            .context("Error fetching alias details")?;
        let res: GetAliasDetailsResponse = assert_response!(res);
        Ok(res.alias)
    }

    /// Replaces the mailboxes an existing alias forwards to. Each entry can be a mailbox email
    /// address or a numeric mailbox ID. Other alias properties are left untouched. Returns the
    /// alias details after the change.
    pub async fn set_alias_mailboxes(
        &self,
        share_id: &ShareId,
        item_id: &ItemId,
        mailboxes: &[String],
    ) -> Result<AliasDetails> {
        self.action_guard(PermissionAction::UpdateItem {
            share_id: share_id.clone(),
            item_id: item_id.clone(),
        })
        .await?;

        let details = self
            .get_alias_details(share_id, item_id)
            .await
            .context("Error fetching current alias mailboxes")?;
        let mut available = details.available_mailboxes;
        for mailbox in details.mailboxes {
            if !available.iter().any(|m| m.id == mailbox.id) {
                available.push(mailbox);
            }
        }
        let mailbox_ids = resolve_mailbox_ids(&available, mailboxes)?;

        let req = POST!("/pass/v1/share/{share_id}/alias/{item_id}/mailbox")
            .body_json(SetAliasMailboxesRequest { mailbox_ids })
            .context("Error serializing set alias mailboxes request")?;
        let res = self
            .send(req)
            .await
            .context("Error sending set alias mailboxes request")?;
        let _: serde_json::Value = assert_response!(res);

        self.get_alias_details(share_id, item_id)
            .await
            .context("Error fetching updated alias mailboxes")
    }
}
