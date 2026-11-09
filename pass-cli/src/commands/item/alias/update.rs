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

use crate::commands::OutputFormat;
use crate::commands::item::agent_monitor::{
    ensure_reason_if_agent, send_reason_if_agent_with_name,
};
use crate::commands::item::common::{ItemQuery, ShareQuery};
use crate::helpers::CliPassClient as PassClient;
use anyhow::{Context, Result, bail};
use pass::AliasMailbox;
use pass::domain::{EventAction, ItemContent, ItemId};

#[derive(serde::Serialize)]
struct JsonAliasMailboxes {
    id: ItemId,
    alias: String,
    mailboxes: Vec<AliasMailbox>,
}

pub async fn run(
    client: PassClient,
    share_query: ShareQuery,
    item_query: ItemQuery,
    mailboxes: Vec<String>,
    output: OutputFormat,
) -> Result<()> {
    ensure_reason_if_agent(&client)?;

    let share_id = share_query.share_id(&client).await?;
    let item_id = item_query.item_id(&share_id, &client).await?;
    let item = client
        .view_item(&share_id, &item_id)
        .await
        .context("Error retrieving item")?;
    if !matches!(item.item.content.content, ItemContent::Alias(_)) {
        bail!("Item {item_id} is not an alias");
    }

    let details = client
        .set_alias_mailboxes(&share_id, &item_id, &mailboxes)
        .await
        .context("Error updating alias mailboxes")?;
    send_reason_if_agent_with_name(
        &client,
        EventAction::ItemUpdate,
        &share_id,
        Some(&item_id),
        Some(&item.item.content.title),
    )
    .await?;

    match output {
        OutputFormat::Human => {
            println!("{}", details.email);
            for mailbox in &details.mailboxes {
                println!("- Mailbox: {}", mailbox.email);
            }
        }
        OutputFormat::Json => {
            let res = JsonAliasMailboxes {
                id: item_id,
                alias: details.email,
                mailboxes: details.mailboxes,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&res).context("Error serializing output")?
            );
        }
    }

    Ok(())
}
