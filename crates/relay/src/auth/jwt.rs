use anyhow::{anyhow, bail, Context};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const ACCESS_TOKEN_TTL_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccessTokenClaims {
    sub: String,
    workspace_id: Uuid,
    iat: i64,
    exp: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAccess {
    pub user_id: Uuid,
    pub workspace_id: Uuid,
}

#[derive(Clone)]
pub struct JwtAccessTokenService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    validation: Validation,
}

impl JwtAccessTokenService {
    pub fn new(secret: &str) -> anyhow::Result<Self> {
        if secret.len() < 32 {
            bail!("jwt secret must be at least 32 characters long");
        }

        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        validation.leeway = 0;
        validation.set_required_spec_claims(&["exp", "sub"]);

        Ok(Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
            validation,
        })
    }

    pub fn issue_workspace_token(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> anyhow::Result<String> {
        self.issue_workspace_token_at(user_id, workspace_id, current_unix_timestamp()?)
    }

    fn issue_workspace_token_at(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
        issued_at: i64,
    ) -> anyhow::Result<String> {
        let claims = AccessTokenClaims {
            sub: user_id.to_string(),
            workspace_id,
            iat: issued_at,
            exp: issued_at + ACCESS_TOKEN_TTL_SECONDS,
        };

        encode(&Header::new(Algorithm::HS256), &claims, &self.encoding_key)
            .context("failed to encode access token")
    }

    pub fn validate_workspace_token(&self, token: &str) -> anyhow::Result<WorkspaceAccess> {
        let claims = decode::<AccessTokenClaims>(token, &self.decoding_key, &self.validation)
            .context("failed to decode access token")?
            .claims;

        let user_id = Uuid::parse_str(&claims.sub)
            .with_context(|| format!("access token subject '{}' is not a UUID", claims.sub))?;

        Ok(WorkspaceAccess { user_id, workspace_id: claims.workspace_id })
    }
}

fn current_unix_timestamp() -> anyhow::Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| anyhow!("system clock is before unix epoch: {error}"))?;

    i64::try_from(duration.as_secs()).context("unix timestamp overflow")
}

#[cfg(test)]
mod tests {
    use super::{current_unix_timestamp, JwtAccessTokenService, ACCESS_TOKEN_TTL_SECONDS};
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use serde::Serialize;
    use uuid::Uuid;

    const TEST_SECRET: &str = "scriptum_test_secret_that_is_definitely_long_enough";

    #[test]
    fn issues_and_validates_workspace_scoped_tokens() {
        let service = JwtAccessTokenService::new(TEST_SECRET).expect("service should initialize");
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();

        let token =
            service.issue_workspace_token(user_id, workspace_id).expect("token should be issued");
        let access = service.validate_workspace_token(&token).expect("token should validate");

        assert_eq!(access.user_id, user_id);
        assert_eq!(access.workspace_id, workspace_id);
    }

    #[test]
    fn rejects_tampered_tokens() {
        let service = JwtAccessTokenService::new(TEST_SECRET).expect("service should initialize");
        let token = service
            .issue_workspace_token(Uuid::new_v4(), Uuid::new_v4())
            .expect("token should be issued");
        let tampered = format!("{token}x");

        assert!(service.validate_workspace_token(&tampered).is_err());
    }

    #[test]
    fn rejects_expired_tokens() {
        let service = JwtAccessTokenService::new(TEST_SECRET).expect("service should initialize");
        let issued_at = current_unix_timestamp().expect("current timestamp should resolve")
            - ACCESS_TOKEN_TTL_SECONDS
            - 1;
        let token = service
            .issue_workspace_token_at(Uuid::new_v4(), Uuid::new_v4(), issued_at)
            .expect("token should be issued");

        assert!(service.validate_workspace_token(&token).is_err());
    }

    #[test]
    fn rejects_tokens_with_invalid_subject_claim() {
        #[derive(Serialize)]
        struct InvalidSubjectClaims {
            sub: &'static str,
            workspace_id: Uuid,
            iat: i64,
            exp: i64,
        }

        let service = JwtAccessTokenService::new(TEST_SECRET).expect("service should initialize");
        let now = current_unix_timestamp().expect("current timestamp should resolve");
        let claims = InvalidSubjectClaims {
            sub: "not-a-uuid",
            workspace_id: Uuid::new_v4(),
            iat: now,
            exp: now + ACCESS_TOKEN_TTL_SECONDS,
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_bytes()),
        )
        .expect("token should encode");

        assert!(service.validate_workspace_token(&token).is_err());
    }
}
