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

//! `item alias contact`: the contacts of an alias and their reverse aliases.

use crate::commands::OutputFormat;
use crate::commands::item::agent_monitor::{
    ensure_reason_if_agent, send_reason_if_agent_with_name,
};
use crate::commands::item::common::{ItemQuery, ShareQuery};
use crate::commands::settings_helper;
use crate::helpers::CliPassClient as PassClient;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use pass::domain::{EventAction, ItemContent, ItemId, ShareId};
use pass::{AliasContact, find_alias_contact, validate_contact_email};

/// The alias item the contact command applies to.
#[derive(Args, Debug)]
pub struct AliasSelector {
    #[arg(long, help = "Share ID of the vault containing the alias")]
    pub share_id: Option<String>,
    #[arg(long, help = "Name of the vault containing the alias")]
    pub vault_name: Option<String>,
    #[arg(long, help = "ID of the alias item")]
    pub item_id: Option<String>,
    #[arg(long, help = "Title of the alias item")]
    pub item_title: Option<String>,
}

/// The contact an existing-contact command applies to.
#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
pub struct ContactSelector {
    #[arg(long, value_name = "EMAIL", help = "Email address of the contact")]
    pub contact: Option<String>,
    #[arg(long, value_name = "ID", help = "ID of the contact")]
    pub contact_id: Option<i64>,
}

#[derive(Subcommand, Debug)]
pub enum ContactCommands {
    #[command(
        about = "Create a contact for an alias and print its reverse alias",
        long_about = "Create a contact for an alias and print its reverse alias.\n\n\
                      A contact is a recipient attached to one alias. Proton generates a reverse \
                      alias for it: an address you send to, which delivers your email to the \
                      contact as if it came from the alias, so your real mailbox stays hidden.\n\n\
                      If the alias already has a contact for this email, the existing contact and \
                      reverse alias are returned (\"existed\": true in JSON) and nothing is created."
    )]
    Create {
        #[command(flatten)]
        alias: AliasSelector,
        #[arg(
            long,
            value_name = "EMAIL",
            help = "Email address of the recipient to write to from the alias"
        )]
        contact: String,
        #[arg(long, help = "Display name of the contact")]
        name: Option<String>,
        #[arg(long, help = "Output format", default_value = "human")]
        output: OutputFormat,
    },
    #[command(about = "List the contacts of an alias and their reverse aliases")]
    List {
        #[command(flatten)]
        alias: AliasSelector,
        #[arg(long, help = "Output format", default_value = "human")]
        output: OutputFormat,
    },
    #[command(about = "Delete a contact of an alias, and with it its reverse alias")]
    Delete {
        #[command(flatten)]
        alias: AliasSelector,
        #[command(flatten)]
        selector: ContactSelector,
        #[arg(long, help = "Output format", default_value = "human")]
        output: OutputFormat,
    },
    #[command(about = "Block the emails a contact sends to an alias")]
    Block {
        #[command(flatten)]
        alias: AliasSelector,
        #[command(flatten)]
        selector: ContactSelector,
        #[arg(long, help = "Output format", default_value = "human")]
        output: OutputFormat,
    },
    #[command(about = "Allow the emails a contact sends to an alias again")]
    Allow {
        #[command(flatten)]
        alias: AliasSelector,
        #[command(flatten)]
        selector: ContactSelector,
        #[arg(long, help = "Output format", default_value = "human")]
        output: OutputFormat,
    },
}

#[derive(Debug, serde::Serialize)]
struct JsonContact {
    contact_id: i64,
    /// Real email address of the contact
    contact: String,
    name: Option<String>,
    /// Address to send to: the contact receives the email as coming from the alias
    reverse_alias_address: String,
    /// Reverse alias in `Name <address>` form, only when the API returns it in that form
    #[serde(skip_serializing_if = "Option::is_none")]
    reverse_alias: Option<String>,
    blocked: bool,
    create_time: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    forwarded_emails: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    replied_emails: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocked_emails: Option<i64>,
}

impl From<&AliasContact> for JsonContact {
    fn from(contact: &AliasContact) -> Self {
        Self {
            contact_id: contact.id,
            contact: contact.email.clone(),
            name: contact.name.clone().filter(|name| !name.is_empty()),
            reverse_alias_address: contact.reverse_alias_address().to_string(),
            reverse_alias: contact.reverse_alias_formatted().map(str::to_string),
            blocked: contact.blocked,
            create_time: contact.create_time,
            forwarded_emails: contact.forwarded_emails,
            replied_emails: contact.replied_emails,
            blocked_emails: contact.blocked_emails,
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct JsonContactResult {
    item_id: ItemId,
    alias: Option<String>,
    #[serde(flatten)]
    contact: JsonContact,
    #[serde(skip_serializing_if = "Option::is_none")]
    existed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct JsonContactList {
    item_id: ItemId,
    alias: Option<String>,
    contacts: Vec<JsonContact>,
}

/// The alias item a command applies to, checked to be an alias.
struct ResolvedAlias {
    share_id: ShareId,
    item_id: ItemId,
    title: String,
    email: Option<String>,
}

async fn resolve_alias(client: &PassClient, selector: AliasSelector) -> Result<ResolvedAlias> {
    let AliasSelector {
        mut share_id,
        vault_name,
        item_id,
        item_title,
    } = selector;
    // Validate the item selection before any request
    let item_query = ItemQuery::new(item_id, item_title)?;
    // Apply default vault if both are None
    if share_id.is_none() && vault_name.is_none() {
        share_id = settings_helper::get_default_share_id(client)
            .await?
            .map(|id| id.to_string());
    }
    let share_query = ShareQuery::new(share_id, vault_name)?;
    let share_id = share_query.share_id(client).await?;
    let item_id = item_query.item_id(&share_id, client).await?;
    let item = client
        .view_item(&share_id, &item_id)
        .await
        .context("Error retrieving item")?;
    if !matches!(item.item.content.content, ItemContent::Alias(_)) {
        bail!("Item {item_id} is not an alias");
    }
    Ok(ResolvedAlias {
        share_id,
        item_id,
        title: item.item.content.title,
        email: item.item.alias_email,
    })
}

async fn resolve_contact(
    client: &PassClient,
    alias: &ResolvedAlias,
    selector: &ContactSelector,
) -> Result<AliasContact> {
    let contacts = client
        .list_alias_contacts(&alias.share_id, &alias.item_id)
        .await?;
    select_contact(&contacts, selector).cloned()
}

fn select_contact<'a>(
    contacts: &'a [AliasContact],
    selector: &ContactSelector,
) -> Result<&'a AliasContact> {
    match (&selector.contact, selector.contact_id) {
        (Some(email), None) => find_alias_contact(contacts, email)
            .ok_or_else(|| anyhow!("The alias has no contact for {}", email.trim())),
        (None, Some(id)) => contacts
            .iter()
            .find(|contact| contact.id == id)
            .ok_or_else(|| anyhow!("The alias has no contact with ID {id}")),
        _ => bail!("Please provide either --contact or --contact-id"),
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).context("Error serializing output")?
    );
    Ok(())
}

fn human_contact_line(contact: &AliasContact) -> String {
    let mut line = format!(
        "- {} -> {} (ID {})",
        contact.email,
        contact.reverse_alias_address(),
        contact.id
    );
    if contact.blocked {
        line.push_str(" [blocked]");
    }
    line
}

fn contact_result(
    alias: &ResolvedAlias,
    contact: &AliasContact,
    existed: Option<bool>,
    deleted: Option<bool>,
) -> JsonContactResult {
    JsonContactResult {
        item_id: alias.item_id.clone(),
        alias: alias.email.clone(),
        contact: JsonContact::from(contact),
        existed,
        deleted,
    }
}

pub async fn run(command: ContactCommands, client: PassClient) -> Result<()> {
    match command {
        ContactCommands::Create {
            alias,
            contact,
            name,
            output,
        } => create(client, alias, contact, name, output).await,
        ContactCommands::List { alias, output } => list(client, alias, output).await,
        ContactCommands::Delete {
            alias,
            selector,
            output,
        } => delete(client, alias, selector, output).await,
        ContactCommands::Block {
            alias,
            selector,
            output,
        } => set_blocked(client, alias, selector, true, output).await,
        ContactCommands::Allow {
            alias,
            selector,
            output,
        } => set_blocked(client, alias, selector, false, output).await,
    }
}

async fn create(
    client: PassClient,
    alias: AliasSelector,
    contact: String,
    name: Option<String>,
    output: OutputFormat,
) -> Result<()> {
    // Fail on a bad address or a missing agent reason before any request
    let contact = validate_contact_email(&contact)?;
    ensure_reason_if_agent(&client)?;

    let alias = resolve_alias(&client, alias).await?;
    let created = client
        .create_alias_contact(&alias.share_id, &alias.item_id, &contact, name.as_deref())
        .await?;
    let action = if created.existed {
        EventAction::ItemRead
    } else {
        EventAction::ItemUpdate
    };
    send_reason_if_agent_with_name(
        &client,
        action,
        &alias.share_id,
        Some(&alias.item_id),
        Some(&alias.title),
    )
    .await?;

    match output {
        OutputFormat::Human => {
            let contact = &created.contact;
            println!("{}", contact.reverse_alias_address());
            let state = if created.existed {
                "existing contact"
            } else {
                "new contact"
            };
            println!("- Contact: {} ({state}, ID {})", contact.email, contact.id);
            if let Some(email) = &alias.email {
                println!(
                    "- Send to the address above to email {} from {email}",
                    contact.email
                );
            }
        }
        OutputFormat::Json => print_json(&contact_result(
            &alias,
            &created.contact,
            Some(created.existed),
            None,
        ))?,
    }
    Ok(())
}

async fn list(client: PassClient, alias: AliasSelector, output: OutputFormat) -> Result<()> {
    ensure_reason_if_agent(&client)?;

    let alias = resolve_alias(&client, alias).await?;
    let contacts = client
        .list_alias_contacts(&alias.share_id, &alias.item_id)
        .await?;
    send_reason_if_agent_with_name(
        &client,
        EventAction::ItemRead,
        &alias.share_id,
        Some(&alias.item_id),
        Some(&alias.title),
    )
    .await?;

    match output {
        OutputFormat::Human => {
            if contacts.is_empty() {
                println!("No contacts");
            }
            for contact in &contacts {
                println!("{}", human_contact_line(contact));
            }
        }
        OutputFormat::Json => print_json(&JsonContactList {
            item_id: alias.item_id,
            alias: alias.email,
            contacts: contacts.iter().map(JsonContact::from).collect(),
        })?,
    }
    Ok(())
}

async fn delete(
    client: PassClient,
    alias: AliasSelector,
    selector: ContactSelector,
    output: OutputFormat,
) -> Result<()> {
    ensure_reason_if_agent(&client)?;

    let alias = resolve_alias(&client, alias).await?;
    let contact = resolve_contact(&client, &alias, &selector).await?;
    client
        .delete_alias_contact(&alias.share_id, &alias.item_id, contact.id)
        .await?;
    send_reason_if_agent_with_name(
        &client,
        EventAction::ItemUpdate,
        &alias.share_id,
        Some(&alias.item_id),
        Some(&alias.title),
    )
    .await?;

    match output {
        OutputFormat::Human => println!("Deleted contact {} (ID {})", contact.email, contact.id),
        OutputFormat::Json => print_json(&contact_result(&alias, &contact, None, Some(true)))?,
    }
    Ok(())
}

async fn set_blocked(
    client: PassClient,
    alias: AliasSelector,
    selector: ContactSelector,
    blocked: bool,
    output: OutputFormat,
) -> Result<()> {
    ensure_reason_if_agent(&client)?;

    let alias = resolve_alias(&client, alias).await?;
    let contact = resolve_contact(&client, &alias, &selector).await?;
    let contact = client
        .set_alias_contact_blocked(&alias.share_id, &alias.item_id, contact.id, blocked)
        .await?;
    send_reason_if_agent_with_name(
        &client,
        EventAction::ItemUpdate,
        &alias.share_id,
        Some(&alias.item_id),
        Some(&alias.title),
    )
    .await?;

    match output {
        OutputFormat::Human => println!("{}", human_contact_line(&contact)),
        OutputFormat::Json => print_json(&contact_result(&alias, &contact, None, None))?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::item::alias::AliasCommands;
    use clap::Parser;

    const RECIPIENT: &str = "recipient@example.com";
    const REVERSE: &str = "recipient_at_example_com_abc@passmail.net";

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: AliasCommands,
    }

    fn parse(args: &[&str]) -> Result<ContactCommands, clap::Error> {
        TestCli::try_parse_from(["alias", "contact"].into_iter().chain(args.iter().copied())).map(
            |cli| match cli.command {
                AliasCommands::Contact { contact_command } => contact_command,
                _ => panic!("expected contact"),
            },
        )
    }

    fn contact(id: i64, email: &str, blocked: bool) -> AliasContact {
        serde_json::from_value(serde_json::json!({
            "ID": id,
            "Name": null,
            "Blocked": blocked,
            "ReverseAlias": REVERSE,
            "Email": email,
            "CreateTime": 1_700_000_000
        }))
        .unwrap()
    }

    #[test]
    fn create_by_title() {
        match parse(&[
            "create",
            "--vault-name",
            "Agent vault",
            "--item-title",
            "Shopping alias",
            "--contact",
            RECIPIENT,
            "--name",
            "Recipient",
            "--output",
            "json",
        ])
        .expect("should parse")
        {
            ContactCommands::Create {
                alias,
                contact,
                name,
                output,
            } => {
                assert_eq!(Some("Agent vault".to_string()), alias.vault_name);
                assert_eq!(Some("Shopping alias".to_string()), alias.item_title);
                assert_eq!(None, alias.item_id);
                assert_eq!(RECIPIENT, contact);
                assert_eq!(Some("Recipient".to_string()), name);
                assert!(matches!(output, OutputFormat::Json));
            }
            other => panic!("expected create, got {other:?}"),
        }
    }

    #[test]
    fn create_by_item_id() {
        match parse(&[
            "create",
            "--share-id",
            "share",
            "--item-id",
            "item",
            "--contact",
            RECIPIENT,
        ])
        .expect("should parse")
        {
            ContactCommands::Create {
                alias,
                name,
                output,
                ..
            } => {
                assert_eq!(Some("share".to_string()), alias.share_id);
                assert_eq!(Some("item".to_string()), alias.item_id);
                assert_eq!(None, alias.item_title);
                assert_eq!(None, name);
                assert!(matches!(output, OutputFormat::Human));
            }
            other => panic!("expected create, got {other:?}"),
        }
    }

    #[test]
    fn create_requires_contact() {
        let err = parse(&["create", "--item-id", "item"]).expect_err("--contact is required");
        assert!(err.to_string().contains("--contact"), "{err}");
    }

    #[test]
    fn item_id_and_title_are_mutually_exclusive() {
        let alias = match parse(&[
            "list",
            "--item-id",
            "item",
            "--item-title",
            "Shopping alias",
        ])
        .expect("clap accepts both, the item query rejects them")
        {
            ContactCommands::List { alias, .. } => alias,
            other => panic!("expected list, got {other:?}"),
        };
        assert!(ItemQuery::new(alias.item_id, alias.item_title).is_err());
        assert!(ItemQuery::new(None, None).is_err());
    }

    #[test]
    fn list_parses() {
        match parse(&["list", "--item-title", "Shopping alias", "--output", "json"])
            .expect("should parse")
        {
            ContactCommands::List { alias, output } => {
                assert_eq!(Some("Shopping alias".to_string()), alias.item_title);
                assert!(matches!(output, OutputFormat::Json));
            }
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn delete_block_allow_need_exactly_one_contact_selector() {
        for command in ["delete", "block", "allow"] {
            assert!(parse(&[command, "--item-id", "item"]).is_err(), "{command}");
            assert!(
                parse(&[
                    command,
                    "--item-id",
                    "item",
                    "--contact",
                    RECIPIENT,
                    "--contact-id",
                    "4"
                ])
                .is_err(),
                "{command}"
            );
            assert!(parse(&[command, "--item-id", "item", "--contact", RECIPIENT]).is_ok());
            assert!(parse(&[command, "--item-id", "item", "--contact-id", "4"]).is_ok());
        }
        assert!(parse(&["delete", "--item-id", "item", "--contact-id", "abc"]).is_err());
    }

    #[test]
    fn selects_contact_by_email_or_id() {
        let contacts = vec![
            contact(1, "other@example.com", false),
            contact(2, RECIPIENT, false),
        ];
        let by_email = ContactSelector {
            contact: Some("Recipient@Example.com".to_string()),
            contact_id: None,
        };
        assert_eq!(2, select_contact(&contacts, &by_email).unwrap().id);
        let by_id = ContactSelector {
            contact: None,
            contact_id: Some(1),
        };
        assert_eq!(1, select_contact(&contacts, &by_id).unwrap().id);
        let unknown = ContactSelector {
            contact: Some("sender@example.com".to_string()),
            contact_id: None,
        };
        assert!(
            select_contact(&contacts, &unknown)
                .unwrap_err()
                .to_string()
                .contains("no contact for sender@example.com")
        );
    }

    #[test]
    fn json_contact_carries_reverse_alias_address() {
        let alias = ResolvedAlias {
            share_id: ShareId::new("share".to_string()),
            item_id: ItemId::new("item".to_string()),
            title: "Shopping alias".to_string(),
            email: Some("alias.example@passmail.net".to_string()),
        };
        let json = serde_json::to_value(contact_result(
            &alias,
            &contact(7, RECIPIENT, false),
            Some(true),
            None,
        ))
        .unwrap();
        assert_eq!(
            serde_json::json!({
                "item_id": "item",
                "alias": "alias.example@passmail.net",
                "contact_id": 7,
                "contact": RECIPIENT,
                "name": null,
                "reverse_alias_address": REVERSE,
                "blocked": false,
                "create_time": 1_700_000_000,
                "existed": true
            }),
            json
        );
    }

    #[test]
    fn json_contact_has_formatted_reverse_alias_only_when_the_api_returns_one() {
        let mut formatted = contact(7, RECIPIENT, true);
        formatted.reverse_alias = format!("Recipient <{REVERSE}>");
        let json = serde_json::to_value(JsonContact::from(&formatted)).unwrap();
        assert_eq!(REVERSE, json["reverse_alias_address"]);
        assert_eq!(format!("Recipient <{REVERSE}>"), json["reverse_alias"]);
        assert_eq!(true, json["blocked"]);
    }

    #[test]
    fn human_line_shows_contact_reverse_alias_and_state() {
        assert_eq!(
            format!("- {RECIPIENT} -> {REVERSE} (ID 3) [blocked]"),
            human_contact_line(&contact(3, RECIPIENT, true))
        );
    }
}
