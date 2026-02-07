use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Maximum UTF-8 byte length for `author_id` in compact binary encoding.
pub const MAX_AUTHOR_ID_LEN: usize = u8::MAX as usize;
const ORIGIN_TAG_FIXED_BYTES: usize = 10; // author_type (1) + author_len (1) + timestamp_millis (8)

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorType {
    Human,
    Agent,
}

impl AuthorType {
    fn to_byte(self) -> u8 {
        match self {
            Self::Human => 0,
            Self::Agent => 1,
        }
    }

    fn from_byte(value: u8) -> Result<Self, OriginTagCodecError> {
        match value {
            0 => Ok(Self::Human),
            1 => Ok(Self::Agent),
            _ => Err(OriginTagCodecError::InvalidAuthorType(value)),
        }
    }
}

impl fmt::Display for AuthorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Human => f.write_str("human"),
            Self::Agent => f.write_str("agent"),
        }
    }
}

/// Structured attribution metadata for CRDT transaction origins.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OriginTag {
    pub author_id: String,
    pub author_type: AuthorType,
    pub timestamp: DateTime<Utc>,
}

impl OriginTag {
    /// Compact encoding for embedding in Yjs transaction origin bytes.
    ///
    /// Layout:
    /// - byte 0: author type (0 = human, 1 = agent)
    /// - byte 1: author_id byte length (0..=255)
    /// - bytes 2..(2+len): UTF-8 author_id
    /// - final 8 bytes: timestamp (UTC millis since epoch, little-endian i64)
    pub fn to_bytes(&self) -> Result<Vec<u8>, OriginTagCodecError> {
        let author_bytes = self.author_id.as_bytes();
        if author_bytes.len() > MAX_AUTHOR_ID_LEN {
            return Err(OriginTagCodecError::AuthorIdTooLong {
                len: author_bytes.len(),
                max: MAX_AUTHOR_ID_LEN,
            });
        }

        let mut encoded = Vec::with_capacity(ORIGIN_TAG_FIXED_BYTES + author_bytes.len());
        encoded.push(self.author_type.to_byte());
        encoded.push(author_bytes.len() as u8);
        encoded.extend_from_slice(author_bytes);
        encoded.extend_from_slice(&self.timestamp.timestamp_millis().to_le_bytes());
        Ok(encoded)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, OriginTagCodecError> {
        if bytes.len() < ORIGIN_TAG_FIXED_BYTES {
            return Err(OriginTagCodecError::PayloadTooShort {
                expected: ORIGIN_TAG_FIXED_BYTES,
                actual: bytes.len(),
            });
        }

        let author_type = AuthorType::from_byte(bytes[0])?;
        let author_len = bytes[1] as usize;
        let expected_len = ORIGIN_TAG_FIXED_BYTES + author_len;
        if bytes.len() != expected_len {
            return Err(OriginTagCodecError::LengthMismatch {
                expected: expected_len,
                actual: bytes.len(),
            });
        }

        let author_start = 2;
        let author_end = author_start + author_len;
        let author_id = String::from_utf8(bytes[author_start..author_end].to_vec())
            .map_err(|_| OriginTagCodecError::InvalidUtf8AuthorId)?;

        let timestamp_millis = i64::from_le_bytes(
            bytes[author_end..author_end + 8].try_into().expect("timestamp slice has fixed length"),
        );
        let timestamp = Utc
            .timestamp_millis_opt(timestamp_millis)
            .single()
            .ok_or(OriginTagCodecError::InvalidTimestampMillis(timestamp_millis))?;

        Ok(Self { author_id, author_type, timestamp })
    }
}

impl fmt::Display for OriginTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}@{}", self.author_type, self.author_id, self.timestamp.to_rfc3339())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OriginTagCodecError {
    #[error("author_id exceeds maximum length ({max} bytes), got {len}")]
    AuthorIdTooLong { len: usize, max: usize },
    #[error("origin payload too short: expected at least {expected} bytes, got {actual}")]
    PayloadTooShort { expected: usize, actual: usize },
    #[error("origin payload length mismatch: expected {expected} bytes, got {actual}")]
    LengthMismatch { expected: usize, actual: usize },
    #[error("invalid author type marker: {0}")]
    InvalidAuthorType(u8),
    #[error("author_id is not valid UTF-8")]
    InvalidUtf8AuthorId,
    #[error("invalid timestamp millis: {0}")]
    InvalidTimestampMillis(i64),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_timestamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 7, 14, 8, 0).single().expect("test timestamp should be valid")
    }

    #[test]
    fn origin_tag_round_trips_in_serde_and_binary_forms() {
        let tag = OriginTag {
            author_id: "claude-agent".to_string(),
            author_type: AuthorType::Agent,
            timestamp: sample_timestamp(),
        };

        let json = serde_json::to_string(&tag).expect("serialize origin tag");
        let decoded_json: OriginTag = serde_json::from_str(&json).expect("deserialize origin tag");
        assert_eq!(decoded_json, tag);

        let bytes = tag.to_bytes().expect("encode origin tag");
        let decoded_bytes = OriginTag::from_bytes(&bytes).expect("decode origin tag");
        assert_eq!(decoded_bytes, tag);
    }

    #[test]
    fn supports_empty_author_id() {
        let tag = OriginTag {
            author_id: String::new(),
            author_type: AuthorType::Human,
            timestamp: sample_timestamp(),
        };

        let bytes = tag.to_bytes().expect("encode origin tag");
        assert_eq!(bytes.len(), ORIGIN_TAG_FIXED_BYTES);
        let decoded = OriginTag::from_bytes(&bytes).expect("decode origin tag");
        assert_eq!(decoded, tag);
    }

    #[test]
    fn supports_max_length_author_id() {
        let tag = OriginTag {
            author_id: "a".repeat(MAX_AUTHOR_ID_LEN),
            author_type: AuthorType::Agent,
            timestamp: sample_timestamp(),
        };

        let bytes = tag.to_bytes().expect("encode origin tag");
        let decoded = OriginTag::from_bytes(&bytes).expect("decode origin tag");
        assert_eq!(decoded.author_id.len(), MAX_AUTHOR_ID_LEN);
        assert_eq!(decoded, tag);
    }

    #[test]
    fn display_includes_type_author_and_timestamp() {
        let tag = OriginTag {
            author_id: "alice".to_string(),
            author_type: AuthorType::Human,
            timestamp: sample_timestamp(),
        };

        assert_eq!(tag.to_string(), format!("human:alice@{}", tag.timestamp.to_rfc3339()));
    }
}
