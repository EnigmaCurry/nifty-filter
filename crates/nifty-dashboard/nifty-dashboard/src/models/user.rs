use crate::models::ids::{IdentityProviderId, SignupMethodId, UserId};
use crate::models::user_status::UserStatus;
use bcrypt::verify as bcrypt_verify;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::types::chrono::NaiveDateTime;
use sqlx::{Error, FromRow, SqlitePool};
use tracing::{debug, info};

use super::identity_provider::IdentityProviders;

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: UserId,
    pub identity_provider_id: IdentityProviderId,
    pub external_id: String,
    pub username: Option<String>,

    pub is_registered: bool,
    pub registered_at: Option<NaiveDateTime>,
    pub signup_method_id: Option<SignupMethodId>,

    pub status: UserStatus,

    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl User {
    /// Verify a plaintext password against this user's stored bcrypt hash.
    ///
    /// Returns:
    ///   - Ok(true)  if the password matches
    ///   - Ok(false) if it doesn't match *or* if the user has no password configured
    ///   - Err(e)    only for database-level errors
    pub async fn verify_password(&self, pool: &SqlitePool, candidate: &str) -> Result<bool, Error> {
        // Fetch the bcrypt hash from the `user_password` table.
        let hash: Option<String> = sqlx::query_scalar(
            r#"
            SELECT password_hash
            FROM user_password
            WHERE user_id = ?1
            "#,
        )
        .bind(self.id.0)
        .fetch_optional(pool)
        .await?;

        // No row => this user doesn't have a local password configured.
        let Some(hash) = hash else {
            return Ok(false);
        };

        // bcrypt::verify errors (malformed hash, etc.) are treated as "no match".
        // If you'd rather bubble them up, change unwrap_or(false) into proper error handling.
        let is_match = bcrypt_verify(candidate, &hash).unwrap_or(false);

        Ok(is_match)
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateUser {
    // shape of whatever your route needs; example:
    pub identity_provider_id: IdentityProviderId,
    pub external_id: String,
    pub username: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PublicUser {
    pub username: String,
}

impl From<User> for PublicUser {
    fn from(u: User) -> Self {
        Self {
            username: u.username.unwrap_or("".to_string()),
        }
    }
}

pub async fn insert_user(pool: &SqlitePool, new_user: CreateUser) -> sqlx::Result<User> {
    sqlx::query_as::<_, User>(
        r#"
        INSERT INTO [user] (
            identity_provider_id,
            external_id,
            username,
            is_registered,
            status
        )
        VALUES (?1, ?2, ?3, 0, 'active')
        RETURNING
            id,
            identity_provider_id,
            external_id,
            username,
            is_registered,
            registered_at,
            signup_method_id,
            status,
            created_at,
            updated_at
        "#,
    )
    .bind(new_user.identity_provider_id)
    .bind(new_user.external_id)
    .bind(new_user.username)
    .fetch_one(pool)
    .await
}

/// Look up a user by its primary‑key.
///
/// Returns:
///   * `Ok(Some(user))` – the row exists.
///   * `Ok(None)`       – no row with that `id`.
///   * `Err(e)`         – any DB‑level error (connection failure, malformed query, …).
#[allow(dead_code)]
pub async fn select_user(pool: &SqlitePool, id: UserId) -> Result<Option<User>, Error> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT
            id,
            identity_provider_id,
            external_id,
            username,
            is_registered,
            registered_at,
            signup_method_id,
            status,
            created_at,
            updated_at
        FROM [user]
        WHERE id = ?1
        "#,
    )
    .bind(id.0)
    .fetch_optional(pool)
    .await
}

/// Look up a user by external OAuth ID.
///
/// Returns:
///   * `Ok(Some(user))` – row exists.
///   * `Ok(None)`       – no row with that external id.
///   * `Err(e)`         – DB error.
pub async fn select_user_by_external_id(
    pool: &SqlitePool,
    external_id: &str,
) -> Result<Option<User>, Error> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT
            id,
            identity_provider_id,
            external_id,
            username,
            is_registered,
            registered_at,
            signup_method_id,
            status,
            created_at,
            updated_at
        FROM [user]
        WHERE external_id = ?1
        "#,
    )
    .bind(external_id)
    .fetch_optional(pool)
    .await
}

/// Look up a user by username
///
/// Returns:
///   * `Ok(Some(user))` – row exists.
///   * `Ok(None)`       – no row with that external id.
///   * `Err(e)`         – DB error.
pub async fn select_user_by_username(
    pool: &SqlitePool,
    username: &str,
) -> Result<Option<User>, Error> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT
            id,
            identity_provider_id,
            external_id,
            username,
            is_registered,
            registered_at,
            signup_method_id,
            status,
            created_at,
            updated_at
        FROM [user]
        WHERE username = ?1
        "#,
    )
    .bind(username)
    .fetch_optional(pool)
    .await
}

/// Get the `identity_providers.id` for the Traefik ForwardAuth provider.
///
/// Adjust the `name` if you used a different code in your seed data.
#[allow(dead_code)]
async fn forwardauth_identity_provider_id(pool: &SqlitePool) -> Result<IdentityProviderId, Error> {
    // Assuming IdentityProviderId is a newtype over i64 / i32.
    let raw_id: i64 = sqlx::query(r#"SELECT id FROM identity_provider WHERE name = ?1"#)
        .bind("traefik-forwardauth")
        .map(|row: sqlx::sqlite::SqliteRow| row.get::<i64, _>("id"))
        .fetch_one(pool)
        .await?;
    debug!(raw_id);
    Ok(IdentityProviderId(raw_id))
}

async fn select_identity_provider_by_name(
    pool: &SqlitePool,
    identity_provider: IdentityProviders,
) -> Result<IdentityProviderId, Error> {
    let raw_id: i64 = sqlx::query(r#"SELECT id FROM identity_provider WHERE name = ?1"#)
        .bind(format!("{:?}", identity_provider))
        .map(|row: sqlx::sqlite::SqliteRow| row.get::<i64, _>("id"))
        .fetch_one(pool)
        .await?;
    Ok(IdentityProviderId(raw_id))
}

/// Get an existing user by e-mail, or create one if it doesn’t exist.
///
/// - Uses the `traefik-forwardauth` identity provider.
pub async fn get_or_create_by_external_id(
    pool: &SqlitePool,
    external_id: &str,
    system_identity_provider: IdentityProviders,
) -> Result<User, Error> {
    // Try to find an existing user first.
    if let Some(user) = select_user_by_external_id(pool, external_id).await? {
        debug!("Found existing user: {user:?}");
        return Ok(user);
    }

    // No existing user – create one.
    info!("Creating user from external id: {external_id:?}");
    let identity_provider_id =
        select_identity_provider_by_name(pool, system_identity_provider).await?;

    let new_user = CreateUser {
        identity_provider_id,
        external_id: external_id.to_string(),
        username: None,
    };

    insert_user(pool, new_user).await
}
