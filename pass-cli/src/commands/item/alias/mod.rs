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

mod create;
mod update;

use crate::commands::OutputFormat;
use crate::commands::item::common::{ItemQuery, ShareQuery};
use crate::commands::settings_helper;
use crate::helpers::CliPassClient as PassClient;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum AliasCommands {
    #[command(about = "Create a new alias")]
    Create {
        #[arg(long, help = "Share ID of the vault where the alias will be created")]
        share_id: Option<String>,
        #[arg(long, help = "Name of the vault where the alias will be created")]
        vault_name: Option<String>,
        #[arg(
            long,
            help = "Prefix of the alias. The resulting email will be [prefix].[suffix]"
        )]
        prefix: String,
        #[arg(
            long = "mailbox",
            value_name = "MAILBOX",
            help = "Mailbox the alias forwards to, as an email address or mailbox ID. Repeat to forward to several mailboxes. Defaults to the account's default mailbox"
        )]
        mailboxes: Vec<String>,
        #[arg(long, help = "Output format", default_value = "human")]
        output: OutputFormat,
    },
    #[command(about = "Change the mailboxes an existing alias forwards to")]
    Update {
        #[arg(long, help = "Share ID of the vault containing the alias")]
        share_id: Option<String>,
        #[arg(long, help = "Name of the vault containing the alias")]
        vault_name: Option<String>,
        #[arg(long, help = "ID of the alias item")]
        item_id: Option<String>,
        #[arg(long, help = "Title of the alias item")]
        item_title: Option<String>,
        #[arg(
            long = "mailbox",
            value_name = "MAILBOX",
            required = true,
            help = "Mailbox the alias forwards to, as an email address or mailbox ID. Repeat to forward to several mailboxes. Replaces the current mailboxes"
        )]
        mailboxes: Vec<String>,
        #[arg(long, help = "Output format", default_value = "human")]
        output: OutputFormat,
    },
}

pub async fn run(subcommand: AliasCommands, client: PassClient) -> Result<()> {
    match subcommand {
        AliasCommands::Create {
            mut share_id,
            vault_name,
            prefix,
            mailboxes,
            output,
        } => {
            // Apply default vault if both are None
            if share_id.is_none() && vault_name.is_none() {
                share_id = settings_helper::get_default_share_id(&client)
                    .await?
                    .map(|id| id.to_string());
            }

            let share_query = ShareQuery::new(share_id, vault_name)?;
            create::run(client, share_query, prefix, mailboxes, output).await
        }
        AliasCommands::Update {
            mut share_id,
            vault_name,
            item_id,
            item_title,
            mailboxes,
            output,
        } => {
            // Apply default vault if both are None
            if share_id.is_none() && vault_name.is_none() {
                share_id = settings_helper::get_default_share_id(&client)
                    .await?
                    .map(|id| id.to_string());
            }

            let share_query = ShareQuery::new(share_id, vault_name)?;
            let item_query = ItemQuery::new(item_id, item_title)?;
            update::run(client, share_query, item_query, mailboxes, output).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: AliasCommands,
    }

    fn parse(args: &[&str]) -> Result<AliasCommands, clap::Error> {
        TestCli::try_parse_from(std::iter::once("alias").chain(args.iter().copied()))
            .map(|cli| cli.command)
    }

    fn create_mailboxes(args: &[&str]) -> Vec<String> {
        match parse(args).expect("should parse") {
            AliasCommands::Create { mailboxes, .. } => mailboxes,
            _ => panic!("expected create"),
        }
    }

    #[test]
    fn create_without_mailbox() {
        assert!(create_mailboxes(&["create", "--prefix", "shop"]).is_empty());
    }

    #[test]
    fn create_with_one_mailbox() {
        assert_eq!(
            vec!["owner@example.com"],
            create_mailboxes(&[
                "create",
                "--prefix",
                "shop",
                "--mailbox",
                "owner@example.com"
            ])
        );
    }

    #[test]
    fn create_with_multiple_mailboxes() {
        assert_eq!(
            vec!["owner@example.com", "second-inbox@example.com", "42"],
            create_mailboxes(&[
                "create",
                "--prefix",
                "shop",
                "--mailbox",
                "owner@example.com",
                "--mailbox=second-inbox@example.com",
                "--mailbox",
                "42",
            ])
        );
    }

    #[test]
    fn update_requires_a_mailbox() {
        assert!(parse(&["update", "--item-id", "abc"]).is_err());
    }

    #[test]
    fn update_with_multiple_mailboxes() {
        match parse(&[
            "update",
            "--item-id",
            "abc",
            "--mailbox",
            "owner@example.com",
            "--mailbox",
            "second-inbox@example.com",
        ])
        .expect("should parse")
        {
            AliasCommands::Update {
                item_id, mailboxes, ..
            } => {
                assert_eq!(Some("abc".to_string()), item_id);
                assert_eq!(
                    vec!["owner@example.com", "second-inbox@example.com"],
                    mailboxes
                );
            }
            _ => panic!("expected update"),
        }
    }
}
