//! Immutable append-only audit event logging.
//!
//! This module intentionally only exposes inserts into `audit_events`.
//! It never stores raw IP addresses or raw user-agent strings.

use std::net::IpAddr;

use anyhow::{anyhow, Context};
use axum::http::{header::USER_AGENT, HeaderMap};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{
    types::chrono::{DateTime, Utc},
    FromRow, PgPool,
};
use uuid::Uuid;

const X_FORWARDED_FOR_HEADER: &str = "x-forwarded-for";
const X_REAL_IP_HEADER: &str = "x-real-ip";
const SHA256_BYTE_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditEventType {
    Auth,
    PermissionChange,
    ShareLinkOperation,
    Delete,
    AdminAction,
}

impl AuditEventType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::PermissionChange => "permission_change",
            Self::ShareLinkOperation => "share_link_operation",
            Self::Delete => "delete",
            Self::AdminAction => "admin_action",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewAuditEvent {
    pub workspace_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub actor_agent_id: Option<String>,
    pub event_type: AuditEventType,
    pub entity_type: String,
    pub entity_id: String,
    pub request_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub details: Option<Value>,
}

#[derive(Debug, Clone, FromRow)]
pub struct AuditEvent {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub actor_agent_id: Option<String>,
    pub event_type: String,
    pub entity_type: String,
    pub entity_id: String,
    pub request_id: Option<String>,
    pub ip_hash: Option<Vec<u8>>,
    pub user_agent_hash: Option<Vec<u8>>,
    pub details: Option<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug)]
struct PreparedAuditEvent {
    workspace_id: Option<Uuid>,
    actor_user_id: Option<Uuid>,
    actor_agent_id: Option<String>,
    event_type: &'static str,
    entity_type: String,
    entity_id: String,
    request_id: Option<String>,
    ip_hash: Option<Vec<u8>>,
    user_agent_hash: Option<Vec<u8>>,
    details: Option<Value>,
}

impl PreparedAuditEvent {
    fn from_new(event: NewAuditEvent) -> anyhow::Result<Self> {
        let entity_type = normalize_required("entity_type", event.entity_type)?;
        let entity_id = normalize_required("entity_id", event.entity_id)?;
        let actor_agent_id = normalize_optional_owned(event.actor_agent_id);
        let request_id = normalize_optional_owned(event.request_id);
        let normalized_ip = normalize_ip(event.ip_address.as_deref());
        let normalized_user_agent = normalize_optional_str(event.user_agent.as_deref());

        Ok(Self {
            workspace_id: event.workspace_id,
            actor_user_id: event.actor_user_id,
            actor_agent_id,
            event_type: event.event_type.as_str(),
            entity_type,
            entity_id,
            request_id,
            ip_hash: hash_pii_value(normalized_ip.as_deref()),
            user_agent_hash: hash_pii_value(normalized_user_agent.as_deref()),
            details: event.details,
        })
    }
}

pub async fn record_event(pool: &PgPool, event: NewAuditEvent) -> anyhow::Result<AuditEvent> {
    let prepared = PreparedAuditEvent::from_new(event)?;

    sqlx::query_as::<_, AuditEvent>(
        r#"
        INSERT INTO audit_events (
            workspace_id,
            actor_user_id,
            actor_agent_id,
            event_type,
            entity_type,
            entity_id,
            request_id,
            ip_hash,
            user_agent_hash,
            details
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING
            id,
            workspace_id,
            actor_user_id,
            actor_agent_id,
            event_type,
            entity_type,
            entity_id,
            request_id,
            ip_hash,
            user_agent_hash,
            details,
            created_at
        "#,
    )
    .bind(prepared.workspace_id)
    .bind(prepared.actor_user_id)
    .bind(prepared.actor_agent_id)
    .bind(prepared.event_type)
    .bind(prepared.entity_type)
    .bind(prepared.entity_id)
    .bind(prepared.request_id)
    .bind(prepared.ip_hash)
    .bind(prepared.user_agent_hash)
    .bind(prepared.details)
    .fetch_one(pool)
    .await
    .context("failed to insert immutable audit event")
}

pub fn client_ip_from_headers(headers: &HeaderMap) -> Option<String> {
    forwarded_for_first_hop(headers)
        .or_else(|| header_value(headers, X_REAL_IP_HEADER))
        .and_then(|value| normalize_ip(Some(value.as_str())))
}

pub fn user_agent_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(USER_AGENT)
        .and_then(|raw| raw.to_str().ok())
        .and_then(|raw| normalize_optional_str(Some(raw)))
}

fn forwarded_for_first_hop(headers: &HeaderMap) -> Option<String> {
    let chain = header_value(headers, X_FORWARDED_FOR_HEADER)?;
    let first_hop = chain.split(',').next()?.trim();
    if first_hop.is_empty() {
        return None;
    }
    Some(first_hop.to_owned())
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|raw| raw.to_str().ok())
        .and_then(|raw| normalize_optional_str(Some(raw)))
}

fn normalize_required(field: &str, value: String) -> anyhow::Result<String> {
    normalize_optional_str(Some(value.as_str())).ok_or_else(|| anyhow!("{field} must not be empty"))
}

fn normalize_optional_owned(value: Option<String>) -> Option<String> {
    value.as_deref().and_then(|raw| normalize_optional_str(Some(raw)))
}

fn normalize_optional_str(value: Option<&str>) -> Option<String> {
    let raw = value?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn normalize_ip(value: Option<&str>) -> Option<String> {
    let raw = normalize_optional_str(value)?;
    let normalized = raw.parse::<IpAddr>().map(|ip| ip.to_string()).unwrap_or(raw);
    Some(normalized)
}

fn hash_pii_value(value: Option<&str>) -> Option<Vec<u8>> {
    let raw = value?;
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    Some(hasher.finalize().to_vec())
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn prepared_event_hashes_ip_and_user_agent_before_insert() {
        let event = NewAuditEvent {
            workspace_id: Some(Uuid::new_v4()),
            actor_user_id: Some(Uuid::new_v4()),
            actor_agent_id: Some("agent-1".to_owned()),
            event_type: AuditEventType::ShareLinkOperation,
            entity_type: "share_link".to_owned(),
            entity_id: "sl_123".to_owned(),
            request_id: Some("req-123".to_owned()),
            ip_address: Some("192.0.2.10".to_owned()),
            user_agent: Some("ScriptumTest/1.0".to_owned()),
            details: Some(serde_json::json!({ "permission": "edit" })),
        };

        let prepared = PreparedAuditEvent::from_new(event).expect("event should validate");
        let expected_ip_hash = hash_pii_value(Some("192.0.2.10")).expect("hash should be present");
        let expected_user_agent_hash =
            hash_pii_value(Some("ScriptumTest/1.0")).expect("hash should be present");

        assert_eq!(prepared.ip_hash, Some(expected_ip_hash.clone()));
        assert_eq!(prepared.user_agent_hash, Some(expected_user_agent_hash.clone()));
        assert_eq!(expected_ip_hash.len(), SHA256_BYTE_LEN);
        assert_eq!(expected_user_agent_hash.len(), SHA256_BYTE_LEN);
        assert_ne!(
            expected_ip_hash, b"192.0.2.10",
            "raw IP must never be persisted in audit_events"
        );
        assert_ne!(
            expected_user_agent_hash, b"ScriptumTest/1.0",
            "raw user-agent must never be persisted in audit_events"
        );
    }

    #[test]
    fn client_ip_prefers_first_forwarded_for_hop() {
        let mut headers = HeaderMap::new();
        headers.insert(
            X_FORWARDED_FOR_HEADER,
            HeaderValue::from_static("198.51.100.42, 198.51.100.8"),
        );
        headers.insert(X_REAL_IP_HEADER, HeaderValue::from_static("203.0.113.9"));

        assert_eq!(client_ip_from_headers(&headers).as_deref(), Some("198.51.100.42"));
    }

    #[test]
    fn prepared_event_rejects_empty_entity_identifier() {
        let event = NewAuditEvent {
            workspace_id: None,
            actor_user_id: None,
            actor_agent_id: None,
            event_type: AuditEventType::AdminAction,
            entity_type: "workspace".to_owned(),
            entity_id: "   ".to_owned(),
            request_id: None,
            ip_address: None,
            user_agent: None,
            details: None,
        };

        let error = PreparedAuditEvent::from_new(event).expect_err("event should be rejected");
        assert!(error.to_string().contains("entity_id"));
    }
}
