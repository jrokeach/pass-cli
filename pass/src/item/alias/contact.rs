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

//! Contacts of an alias.
//!
//! A contact is a recipient attached to one alias. Creating it makes the server generate a
//! reverse alias: a unique address that, when written to, delivers the email to the contact as
//! if it was sent from the alias, so the real mailbox is never exposed. Contacts are addressed
//! by the share ID and item ID of the alias item, the same as the other alias endpoints.

use crate::domain::{AccountType, ItemId, ShareId};
use crate::error::ProtonApiErrorCode;
use crate::permission::PermissionAction;
use crate::{PassClient, PassClientContext};
use anyhow::{Context, Result, anyhow, bail};
use muon::{DELETE, GET, POST, PUT};

/// Upper bound on the pages fetched when listing contacts, as a guard against a server that
/// never signals the end of the list.
const MAX_CONTACT_PAGES: usize = 1000;

/// A contact of an alias, as returned by the alias contact endpoints.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct AliasContact {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Name", default)]
    pub name: Option<String>,
    /// Real email address of the contact
    #[serde(rename = "Email")]
    pub email: String,
    /// Address to write to so the contact receives the email as coming from the alias
    #[serde(rename = "ReverseAlias")]
    pub reverse_alias: String,
    /// Whether emails from this contact to the alias are blocked
    #[serde(rename = "Blocked", default)]
    pub blocked: bool,
    #[serde(rename = "CreateTime", default)]
    pub create_time: i64,
    /// Emails received from this contact in the last 14 days (only in the contact list)
    #[serde(rename = "ForwardedEmails", default)]
    pub forwarded_emails: Option<i64>,
    /// Emails sent to this contact in the last 14 days (only in the contact list)
    #[serde(rename = "RepliedEmails", default)]
    pub replied_emails: Option<i64>,
    /// Emails from this contact blocked in the last 14 days (only in the contact list)
    #[serde(rename = "BlockedEmails", default)]
    pub blocked_emails: Option<i64>,
}

impl AliasContact {
    /// The bare reverse alias address. The API returns a bare address; a `Name <address>` value
    /// is reduced to the address.
    pub fn reverse_alias_address(&self) -> &str {
        split_reverse_alias(&self.reverse_alias).0
    }

    /// The reverse alias in `Name <address>` form, when the API returned it in that form.
    pub fn reverse_alias_formatted(&self) -> Option<&str> {
        split_reverse_alias(&self.reverse_alias).1
    }
}

/// Splits a reverse alias value into its bare address and, if the value was in
/// `Name <address>` form, the formatted value.
fn split_reverse_alias(value: &str) -> (&str, Option<&str>) {
    let value = value.trim();
    match (value.rfind('<'), value.ends_with('>')) {
        (Some(start), true) => (value[start + 1..value.len() - 1].trim(), Some(value)),
        _ => (value, None),
    }
}

/// Result of [`PassClient::create_alias_contact`].
#[derive(Clone, Debug)]
pub struct CreatedAliasContact {
    pub contact: AliasContact,
    /// `true` if the alias already had a contact for this email, which was returned instead of
    /// creating a duplicate.
    pub existed: bool,
}

/// Error returned by the server for an alias contact request.
#[derive(Clone, Debug)]
pub struct AliasContactApiError {
    pub status: u16,
    pub code: Option<i64>,
    pub message: Option<String>,
    pub account_type: AccountType,
}

impl AliasContactApiError {
    fn from_response(
        status: u16,
        body: Option<&serde_json::Value>,
        account_type: AccountType,
    ) -> Self {
        let code = body.and_then(|b| b.get("Code")).and_then(|c| c.as_i64());
        let message = body
            .and_then(|b| b.get("Error"))
            .and_then(|e| e.as_str())
            .map(|e| e.to_string());
        Self {
            status,
            code,
            message,
            account_type,
        }
    }

    /// The server refused the request for lack of permission.
    pub fn is_permission_denied(&self) -> bool {
        self.status == 403
            || matches!(
                self.code_name().as_deref(),
                Some("PermissionDenied" | "InsufficientScope" | "NotAllowed")
            )
    }

    /// The server refused the request because the session is an agent session or a personal
    /// access token, not a full account session.
    pub fn is_refused_for_session_type(&self) -> bool {
        self.is_permission_denied()
            && matches!(
                self.account_type,
                AccountType::AgentSession | AccountType::PersonalAccessToken
            )
    }

    /// The server rejected the request because the contact already exists.
    pub fn is_already_exists(&self) -> bool {
        self.status == 409 || self.code_name().as_deref() == Some("AlreadyExists")
    }

    fn code_name(&self) -> Option<String> {
        let code = self.code?;
        serde_json::from_value::<ProtonApiErrorCode>(serde_json::Value::from(code))
            .ok()
            .map(|c| c.name())
    }

    fn describe(&self) -> String {
        let mut details = format!("HTTP {}", self.status);
        if let Some(code) = self.code {
            details.push_str(&format!(", code {code}"));
            if let Some(name) = self.code_name() {
                details.push_str(&format!(" ({name})"));
            }
        }
        if let Some(message) = &self.message {
            details.push_str(&format!(": {message}"));
        }
        details
    }
}

impl std::fmt::Display for AliasContactApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_refused_for_session_type() {
            let session = match self.account_type {
                AccountType::AgentSession => "an agent session",
                _ => "a personal access token session",
            };
            write!(
                f,
                "The Proton API refused to let {session} manage alias contacts ({}). The session \
                 may lack write access to this vault, or alias contacts may not be available to \
                 {session}",
                self.describe()
            )
        } else if self.is_permission_denied() {
            write!(
                f,
                "The Proton API refused the alias contact request ({})",
                self.describe()
            )
        } else {
            write!(f, "Alias contact request failed ({})", self.describe())
        }
    }
}

impl std::error::Error for AliasContactApiError {}

#[derive(serde::Deserialize)]
struct ListAliasContactsResponse {
    #[serde(rename = "Contacts", default)]
    contacts: Vec<AliasContact>,
    #[serde(rename = "LastID", default)]
    last_id: Option<i64>,
}

#[derive(serde::Deserialize)]
struct AliasContactResponse {
    #[serde(rename = "Contact", default)]
    contact: Option<AliasContact>,
}

#[derive(Debug, serde::Serialize)]
struct CreateAliasContactRequest {
    #[serde(rename = "Email")]
    email: String,
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(serde::Serialize)]
struct SetAliasContactBlockedRequest {
    #[serde(rename = "Blocked")]
    blocked: bool,
}

/// Checks that `email` looks like a valid email address, returning it trimmed. This is a
/// sanity check before calling the API, which does its own validation.
pub fn validate_contact_email(email: &str) -> Result<String> {
    let email = email.trim();
    if email.is_empty() {
        bail!("The contact email is empty");
    }
    let invalid = |reason: &str| anyhow!("Invalid contact email '{email}': {reason}");
    if email.len() > 254 {
        return Err(invalid("longer than 254 characters"));
    }
    if email
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || "<>()[]\\,;:\"".contains(c))
    {
        return Err(invalid(
            "expected a bare address such as recipient@example.com (set the display name with --name)",
        ));
    }
    let (local, domain) = email
        .split_once('@')
        .ok_or_else(|| invalid("missing '@'"))?;
    if domain.contains('@') {
        return Err(invalid("more than one '@'"));
    }
    if local.is_empty() || local.len() > 64 {
        return Err(invalid("the part before '@' must be 1 to 64 characters"));
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return Err(invalid("misplaced '.' before '@'"));
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return Err(invalid("the domain needs at least one '.'"));
    }
    for label in &labels {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label.chars().all(|c| c.is_alphanumeric() || c == '-')
        {
            return Err(invalid("invalid domain"));
        }
    }
    let tld = labels.last().copied().unwrap_or_default();
    if tld.len() < 2 || tld.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid("invalid top-level domain"));
    }
    Ok(email.to_string())
}

/// Finds the contact for `email` (case-insensitive) in `contacts`.
pub fn find_alias_contact<'a>(
    contacts: &'a [AliasContact],
    email: &str,
) -> Option<&'a AliasContact> {
    let email = email.trim();
    contacts
        .iter()
        .find(|contact| contact.email.trim().eq_ignore_ascii_case(email))
}

impl<C: PassClientContext> PassClient<C> {
    async fn send_alias_contact_request<T: serde::de::DeserializeOwned>(
        &self,
        req: muon::http::HttpReq,
    ) -> Result<T> {
        let res = self.send(req).await?;
        if !res.status().is_success() {
            crate::utils::debug_response(&res);
            let body = res.body_json::<serde_json::Value>().ok();
            return Err(AliasContactApiError::from_response(
                res.status().as_u16(),
                body.as_ref(),
                self.account_type(),
            )
            .into());
        }
        res.body_json()
            .map_err(|e| anyhow!("Error parsing response body: {e}"))
    }

    /// Lists all the contacts of an alias item.
    pub async fn list_alias_contacts(
        &self,
        share_id: &ShareId,
        item_id: &ItemId,
    ) -> Result<Vec<AliasContact>> {
        let mut contacts = Vec::new();
        let mut since: Option<i64> = None;
        for _ in 0..MAX_CONTACT_PAGES {
            let mut req = GET!("/pass/v1/share/{share_id}/alias/{item_id}/contact");
            if let Some(since) = since {
                req = req.query(("Since", since.to_string()));
            }
            let page: ListAliasContactsResponse = self
                .send_alias_contact_request(req)
                .await
                .context("Error listing alias contacts")?;
            let page_is_empty = page.contacts.is_empty();
            contacts.extend(page.contacts);
            match page.last_id {
                Some(last_id) if last_id != 0 && !page_is_empty && Some(last_id) != since => {
                    since = Some(last_id);
                }
                _ => return Ok(contacts),
            }
        }
        Ok(contacts)
    }

    /// Fetches one contact of an alias item.
    pub async fn get_alias_contact(
        &self,
        share_id: &ShareId,
        item_id: &ItemId,
        contact_id: i64,
    ) -> Result<AliasContact> {
        let res: AliasContactResponse = self
            .send_alias_contact_request(GET!(
                "/pass/v1/share/{share_id}/alias/{item_id}/contact/{contact_id}"
            ))
            .await
            .context("Error fetching alias contact")?;
        res.contact.context("The response has no contact")
    }

    /// Creates a contact for `email` on an alias item and returns it with its reverse alias.
    /// If the alias already has a contact for that email, that contact is returned with
    /// `existed` set and nothing is created.
    pub async fn create_alias_contact(
        &self,
        share_id: &ShareId,
        item_id: &ItemId,
        email: &str,
        name: Option<&str>,
    ) -> Result<CreatedAliasContact> {
        let email = validate_contact_email(email)?;
        self.action_guard(PermissionAction::UpdateItem {
            share_id: share_id.clone(),
            item_id: item_id.clone(),
        })
        .await?;

        let existing = self.list_alias_contacts(share_id, item_id).await?;
        if let Some(contact) = find_alias_contact(&existing, &email) {
            return Ok(CreatedAliasContact {
                contact: contact.clone(),
                existed: true,
            });
        }

        let req = POST!("/pass/v1/share/{share_id}/alias/{item_id}/contact")
            .body_json(build_create_alias_contact_request(&email, name))
            .context("Error serializing create alias contact request")?;
        match self
            .send_alias_contact_request::<AliasContactResponse>(req)
            .await
        {
            Ok(res) => Ok(CreatedAliasContact {
                contact: res.contact.context("The response has no contact")?,
                existed: false,
            }),
            Err(err) => {
                // The contact may have been created since it was listed: return it if so
                let retry = err.downcast_ref::<AliasContactApiError>().is_some_and(|e| {
                    e.is_already_exists()
                        || (400..500).contains(&e.status) && !e.is_permission_denied()
                });
                if retry {
                    let contacts = self.list_alias_contacts(share_id, item_id).await?;
                    if let Some(contact) = find_alias_contact(&contacts, &email) {
                        return Ok(CreatedAliasContact {
                            contact: contact.clone(),
                            existed: true,
                        });
                    }
                }
                Err(err.context("Error creating alias contact"))
            }
        }
    }

    /// Deletes a contact of an alias item, and with it its reverse alias.
    pub async fn delete_alias_contact(
        &self,
        share_id: &ShareId,
        item_id: &ItemId,
        contact_id: i64,
    ) -> Result<()> {
        self.action_guard(PermissionAction::UpdateItem {
            share_id: share_id.clone(),
            item_id: item_id.clone(),
        })
        .await?;
        let _: serde_json::Value = self
            .send_alias_contact_request(DELETE!(
                "/pass/v1/share/{share_id}/alias/{item_id}/contact/{contact_id}"
            ))
            .await
            .context("Error deleting alias contact")?;
        Ok(())
    }

    /// Blocks or allows the emails a contact sends to an alias item. Returns the contact after
    /// the change.
    pub async fn set_alias_contact_blocked(
        &self,
        share_id: &ShareId,
        item_id: &ItemId,
        contact_id: i64,
        blocked: bool,
    ) -> Result<AliasContact> {
        self.action_guard(PermissionAction::UpdateItem {
            share_id: share_id.clone(),
            item_id: item_id.clone(),
        })
        .await?;
        let req = PUT!("/pass/v1/share/{share_id}/alias/{item_id}/contact/{contact_id}/blocked")
            .body_json(SetAliasContactBlockedRequest { blocked })
            .context("Error serializing alias contact blocked request")?;
        let res: AliasContactResponse = self
            .send_alias_contact_request(req)
            .await
            .context("Error updating alias contact")?;
        match res.contact {
            Some(contact) => Ok(contact),
            None => self.get_alias_contact(share_id, item_id, contact_id).await,
        }
    }
}

fn build_create_alias_contact_request(
    email: &str,
    name: Option<&str>,
) -> CreateAliasContactRequest {
    CreateAliasContactRequest {
        email: email.to_string(),
        name: name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tools::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SHARE_ID: &str = "MyShareID";
    const ITEM_ID: &str = "MyItemID";
    const RECIPIENT: &str = "recipient@example.com";
    const REVERSE: &str = "recipient_at_example_com_abc@passmail.net";

    fn contact_json(id: i64, email: &str, reverse: &str) -> serde_json::Value {
        serde_json::json!({
            "ID": id,
            "Name": null,
            "Blocked": false,
            "ReverseAlias": reverse,
            "Email": email,
            "CreateTime": 1_700_000_000,
        })
    }

    fn contact(id: i64, email: &str) -> AliasContact {
        serde_json::from_value(contact_json(id, email, REVERSE)).unwrap()
    }

    /// The test server drops a handler once it has answered, so register a few copies of the
    /// handlers that answer more than once.
    fn handle_repeatedly<F>(api: &ProtonAPI, method: Method, path: String, handler: F)
    where
        F: Fn(&muon_test::server::Request) -> Option<muon_test::server::Response>
            + Clone
            + Send
            + Sync
            + 'static,
    {
        for _ in 0..5 {
            api.handler_with_method(method, path.clone(), handler.clone());
        }
    }

    fn contacts_path() -> String {
        format!("/pass/v1/share/{SHARE_ID}/alias/{ITEM_ID}/contact")
    }

    fn api_error(
        status: u16,
        code: Option<i64>,
        account_type: AccountType,
    ) -> AliasContactApiError {
        let body = code.map(|code| serde_json::json!({"Code": code, "Error": "Server message"}));
        AliasContactApiError::from_response(status, body.as_ref(), account_type)
    }

    // --- email validation

    #[test]
    fn valid_emails_are_accepted_and_trimmed() {
        for email in [
            "recipient@example.com",
            "first.last+tag@sub.example.co.uk",
            "x@example.io",
            "o'brien@example.xn--p1ai",
        ] {
            assert_eq!(email, validate_contact_email(email).expect(email));
        }
        assert_eq!(
            RECIPIENT,
            validate_contact_email("  recipient@example.com \n").unwrap()
        );
    }

    #[test]
    fn invalid_emails_are_rejected() {
        for email in [
            "",
            "   ",
            "recipient",
            "recipient@",
            "@example.com",
            "a@b@example.com",
            "recipient@localhost",
            "recipient@example.",
            "recipient@.example.com",
            "recipient@-example.com",
            "recipient@exa_mple.com",
            "recipient@example.c",
            "recipient@127.0.0.1",
            ".recipient@example.com",
            "re..cipient@example.com",
            "recip ient@example.com",
            "Recipient <recipient@example.com>",
            "recipient@example.com,sender@example.com",
        ] {
            assert!(
                validate_contact_email(email).is_err(),
                "{email:?} should be rejected"
            );
        }
    }

    #[test]
    fn formatted_address_error_suggests_name_option() {
        let err = validate_contact_email("Recipient <recipient@example.com>").unwrap_err();
        assert!(err.to_string().contains("--name"), "{err}");
    }

    // --- response parsing

    #[test]
    fn parses_contact_list_with_stats() {
        let res: ListAliasContactsResponse = serde_json::from_value(serde_json::json!({
            "Code": 1000,
            "Contacts": [{
                "ID": 7,
                "Name": "Recipient",
                "Blocked": true,
                "ReverseAlias": REVERSE,
                "Email": RECIPIENT,
                "CreateTime": 1_700_000_000,
                "RepliedEmails": 1,
                "ForwardedEmails": 2,
                "BlockedEmails": 3
            }],
            "Total": 1,
            "LastID": 7
        }))
        .unwrap();
        assert_eq!(Some(7), res.last_id);
        let contact = &res.contacts[0];
        assert_eq!(7, contact.id);
        assert_eq!(Some("Recipient".to_string()), contact.name);
        assert!(contact.blocked);
        assert_eq!(RECIPIENT, contact.email);
        assert_eq!(REVERSE, contact.reverse_alias_address());
        assert_eq!(None, contact.reverse_alias_formatted());
        assert_eq!(Some(2), contact.forwarded_emails);
        assert_eq!(Some(1), contact.replied_emails);
        assert_eq!(Some(3), contact.blocked_emails);
    }

    #[test]
    fn parses_created_contact_without_stats() {
        let res: AliasContactResponse = serde_json::from_value(serde_json::json!({
            "Code": 1000,
            "Contact": contact_json(9, RECIPIENT, REVERSE)
        }))
        .unwrap();
        let contact = res.contact.unwrap();
        assert_eq!(9, contact.id);
        assert_eq!(None, contact.name);
        assert_eq!(None, contact.forwarded_emails);
    }

    #[test]
    fn reverse_alias_address_is_extracted_from_formatted_value() {
        let contact: AliasContact = serde_json::from_value(contact_json(
            1,
            RECIPIENT,
            &format!("\"Recipient\" <{REVERSE}>"),
        ))
        .unwrap();
        assert_eq!(REVERSE, contact.reverse_alias_address());
        assert_eq!(
            Some(format!("\"Recipient\" <{REVERSE}>").as_str()),
            contact.reverse_alias_formatted()
        );
    }

    #[test]
    fn finds_contact_case_insensitively() {
        let contacts = vec![
            contact(1, "other@example.com"),
            contact(2, "Recipient@Example.com"),
        ];
        assert_eq!(
            2,
            find_alias_contact(&contacts, " recipient@example.com")
                .unwrap()
                .id
        );
        assert!(find_alias_contact(&contacts, "sender@example.com").is_none());
    }

    // --- request building

    #[test]
    fn create_request_carries_email_and_name() {
        let req = build_create_alias_contact_request(RECIPIENT, Some(" Recipient "));
        assert_eq!(
            serde_json::json!({"Email": RECIPIENT, "Name": "Recipient"}),
            serde_json::to_value(&req).unwrap()
        );
    }

    #[test]
    fn create_request_omits_missing_or_blank_name() {
        for name in [None, Some(""), Some("  ")] {
            let req = build_create_alias_contact_request(RECIPIENT, name);
            assert_eq!(
                serde_json::json!({"Email": RECIPIENT}),
                serde_json::to_value(&req).unwrap()
            );
        }
    }

    // --- error mapping

    #[test]
    fn forbidden_for_agent_session_has_distinct_message() {
        let err = api_error(403, Some(2026), AccountType::AgentSession);
        assert!(err.is_refused_for_session_type());
        let msg = err.to_string();
        assert!(
            msg.contains("refused to let an agent session manage alias contacts"),
            "{msg}"
        );
        assert!(
            msg.contains("HTTP 403, code 2026 (PermissionDenied): Server message"),
            "{msg}"
        );
    }

    #[test]
    fn scope_error_for_pat_session_is_refused_for_session_type() {
        let err = api_error(422, Some(2027), AccountType::PersonalAccessToken);
        assert!(err.is_refused_for_session_type());
        assert!(
            err.to_string().contains("a personal access token session"),
            "{err}"
        );
    }

    #[test]
    fn forbidden_for_user_session_is_not_blamed_on_session_type() {
        let err = api_error(403, None, AccountType::User);
        assert!(err.is_permission_denied());
        assert!(!err.is_refused_for_session_type());
        let msg = err.to_string();
        assert!(
            msg.starts_with("The Proton API refused the alias contact request"),
            "{msg}"
        );
        assert!(!msg.contains("agent"), "{msg}");
    }

    #[test]
    fn other_errors_are_reported_with_status_and_code() {
        let err = api_error(422, Some(2500), AccountType::AgentSession);
        assert!(err.is_already_exists());
        assert!(!err.is_refused_for_session_type());
        assert!(
            err.to_string()
                .contains("HTTP 422, code 2500 (AlreadyExists)"),
            "{err}"
        );

        let err = api_error(500, Some(123_456), AccountType::User);
        assert_eq!(
            "Alias contact request failed (HTTP 500, code 123456: Server message)",
            err.to_string()
        );
    }

    // --- requests against the API

    async fn user_client(server: muon_test::Server) -> (TestPassClient, ProtonAPI) {
        let (raw_client, api) = server.client::<()>();
        let client = make_test_pass_client_with_setup(raw_client, &api, PlanType::Plus).await;
        setup_share_keys(&api, SHARE_ID);
        setup_vault_share(&api, SHARE_ID);
        (client, api)
    }

    #[muon_test::test]
    async fn test_list_follows_pages(server: muon_test::Server) {
        let (client, api) = user_client(server).await;
        handle_repeatedly(&api, Method::GET, contacts_path(), |req| {
            match req.uri().query().unwrap_or_default() {
                "" => success(serde_json::json!({
                    "Code": 1000,
                    "Contacts": [contact_json(1, "a@example.com", REVERSE), contact_json(2, "b@example.com", REVERSE)],
                    "Total": 3,
                    "LastID": 2
                })),
                "Since=2" => success(serde_json::json!({
                    "Code": 1000,
                    "Contacts": [contact_json(3, "c@example.com", REVERSE)],
                    "Total": 3,
                    "LastID": 3
                })),
                "Since=3" => success(serde_json::json!({
                    "Code": 1000, "Contacts": [], "Total": 3, "LastID": 0
                })),
                other => panic!("unexpected query {other}"),
            }
        });

        let contacts = client
            .list_alias_contacts(&share_id!(SHARE_ID), &item_id!(ITEM_ID))
            .await
            .expect("Should list contacts");
        let ids: Vec<i64> = contacts.iter().map(|c| c.id).collect();
        assert_eq!(vec![1, 2, 3], ids);
    }

    #[muon_test::test]
    async fn test_create_new_contact(server: muon_test::Server) {
        let (client, api) = user_client(server).await;
        api.handler_with_method(Method::GET, contacts_path(), |_| {
            success(serde_json::json!({"Code": 1000, "Contacts": [], "Total": 0, "LastID": 0}))
        });
        let created = api.handler_with_method(Method::POST, contacts_path(), |_| {
            success(
                serde_json::json!({"Code": 1000, "Contact": contact_json(5, RECIPIENT, REVERSE)}),
            )
        });
        let recorder = api.new_recorder();

        let res = client
            .create_alias_contact(
                &share_id!(SHARE_ID),
                &item_id!(ITEM_ID),
                " recipient@example.com ",
                Some("Recipient"),
            )
            .await
            .expect("Should create contact");

        assert_hit!(created);
        assert!(!res.existed);
        assert_eq!(5, res.contact.id);
        assert_eq!(REVERSE, res.contact.reverse_alias_address());
        let request: serde_json::Value = last_request!(recorder);
        assert_eq!(
            serde_json::json!({"Email": RECIPIENT, "Name": "Recipient"}),
            request
        );
    }

    #[muon_test::test]
    async fn test_create_existing_contact_returns_it(server: muon_test::Server) {
        let (client, api) = user_client(server).await;
        api.handler_with_method(Method::GET, contacts_path(), |_| {
            success(serde_json::json!({
                "Code": 1000,
                "Contacts": [contact_json(4, "Recipient@Example.com", REVERSE)],
                "Total": 1,
                "LastID": 0
            }))
        });
        let created = api.handler_with_method(Method::POST, contacts_path(), |_| {
            panic!("an existing contact must not be created again")
        });

        let res = client
            .create_alias_contact(&share_id!(SHARE_ID), &item_id!(ITEM_ID), RECIPIENT, None)
            .await
            .expect("Should return existing contact");

        assert_not_hit!(created);
        assert!(res.existed);
        assert_eq!(4, res.contact.id);
        assert_eq!(REVERSE, res.contact.reverse_alias_address());
    }

    #[muon_test::test]
    async fn test_create_conflict_returns_contact_created_meanwhile(server: muon_test::Server) {
        let (client, api) = user_client(server).await;
        let lists = Arc::new(AtomicUsize::new(0));
        let lists_clone = lists.clone();
        handle_repeatedly(&api, Method::GET, contacts_path(), move |_| {
            let contacts = if lists_clone.fetch_add(1, Ordering::SeqCst) == 0 {
                serde_json::json!([])
            } else {
                serde_json::json!([contact_json(6, RECIPIENT, REVERSE)])
            };
            success(
                serde_json::json!({"Code": 1000, "Contacts": contacts, "Total": 1, "LastID": 0}),
            )
        });
        api.handler_with_method(Method::POST, contacts_path(), |_| {
            response_from_status_and_payload(
                422,
                serde_json::json!({"Code": 2500, "Error": "Contact already exists"}),
            )
        });

        let res = client
            .create_alias_contact(&share_id!(SHARE_ID), &item_id!(ITEM_ID), RECIPIENT, None)
            .await
            .expect("Should return the contact created meanwhile");

        assert!(res.existed);
        assert_eq!(6, res.contact.id);
        assert_eq!(2, lists.load(Ordering::SeqCst));
    }

    #[muon_test::test]
    async fn test_create_error_is_reported_when_contact_is_still_missing(
        server: muon_test::Server,
    ) {
        let (client, api) = user_client(server).await;
        handle_repeatedly(&api, Method::GET, contacts_path(), |_| {
            success(serde_json::json!({"Code": 1000, "Contacts": [], "Total": 0, "LastID": 0}))
        });
        api.handler_with_method(Method::POST, contacts_path(), |_| {
            response_from_status_and_payload(
                422,
                serde_json::json!({"Code": 2001, "Error": "Invalid email"}),
            )
        });

        let err = client
            .create_alias_contact(&share_id!(SHARE_ID), &item_id!(ITEM_ID), RECIPIENT, None)
            .await
            .expect_err("Should fail");
        let msg = format!("{err:#}");
        assert!(msg.contains("Error creating alias contact"), "{msg}");
        assert!(
            msg.contains("HTTP 422, code 2001 (InvalidValue): Invalid email"),
            "{msg}"
        );
    }

    #[muon_test::test]
    async fn test_invalid_email_is_rejected_before_any_request(server: muon_test::Server) {
        let (client, api) = user_client(server).await;
        let listed = api.handler_with_method(Method::GET, contacts_path(), |_| {
            success(serde_json::json!({"Code": 1000, "Contacts": [], "Total": 0, "LastID": 0}))
        });

        client
            .create_alias_contact(&share_id!(SHARE_ID), &item_id!(ITEM_ID), "recipient", None)
            .await
            .expect_err("Should reject the email");
        assert_not_hit!(listed);
    }

    #[muon_test::test]
    async fn test_refused_pat_session_gets_distinct_error(server: muon_test::Server) {
        let (raw_client, api) = server.client::<()>();
        let client = make_test_pass_pat_client(raw_client, &api).await;
        api.handler_with_method(Method::GET, contacts_path(), |_| {
            response_from_status_and_payload(
                403,
                serde_json::json!({"Code": 2026, "Error": "Permission denied"}),
            )
        });

        let err = client
            .list_alias_contacts(&share_id!(SHARE_ID), &item_id!(ITEM_ID))
            .await
            .expect_err("Should be refused");
        let api_err = err
            .downcast_ref::<AliasContactApiError>()
            .expect("Should be an alias contact API error");
        assert!(api_err.is_refused_for_session_type());
        assert_eq!(403, api_err.status);
        let msg = format!("{err:#}");
        assert!(
            msg.contains("refused to let a personal access token session manage alias contacts"),
            "{msg}"
        );
    }

    #[muon_test::test]
    async fn test_delete_contact(server: muon_test::Server) {
        let (client, api) = user_client(server).await;
        let deleted =
            api.handler_with_method(Method::DELETE, format!("{}/5", contacts_path()), |_| {
                success_code()
            });

        client
            .delete_alias_contact(&share_id!(SHARE_ID), &item_id!(ITEM_ID), 5)
            .await
            .expect("Should delete contact");
        assert_hit!(deleted);
    }

    #[muon_test::test]
    async fn test_block_contact(server: muon_test::Server) {
        let (client, api) = user_client(server).await;
        let blocked = api.handler_with_method(
            Method::PUT,
            format!("{}/5/blocked", contacts_path()),
            |_| {
                let mut contact = contact_json(5, RECIPIENT, REVERSE);
                contact["Blocked"] = serde_json::Value::Bool(true);
                success(serde_json::json!({"Code": 1000, "Contact": contact}))
            },
        );
        let recorder = api.new_recorder();

        let contact = client
            .set_alias_contact_blocked(&share_id!(SHARE_ID), &item_id!(ITEM_ID), 5, true)
            .await
            .expect("Should block contact");

        assert_hit!(blocked);
        assert!(contact.blocked);
        let request: serde_json::Value = last_request!(recorder);
        assert_eq!(serde_json::json!({"Blocked": true}), request);
    }
}
