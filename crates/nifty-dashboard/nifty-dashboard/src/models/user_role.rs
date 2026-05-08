use crate::models::ids::UserId;
use sqlx::FromRow;
use sqlx::SqlitePool;

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct UserRole {
    pub user_id: UserId,
    pub role_id: i64,
    pub assigned_at: sqlx::types::chrono::NaiveDateTime,
    pub assigned_by: Option<UserId>,
}

#[allow(dead_code)]
pub async fn user_has_role(
    pool: &SqlitePool,
    user_id: UserId,
    role_name: &str,
) -> Result<bool, sqlx::Error> {
    // This matches your schema:
    //
    // user_role(user_id TEXT, role_id INTEGER)
    // role(id INTEGER, name TEXT)
    let exists: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT 1
        FROM user_role ur
        JOIN [role] r ON r.id = ur.role_id
        WHERE ur.user_id = ?1
          AND r.name = ?2
        "#,
    )
    .bind(user_id)
    .bind(role_name)
    .fetch_optional(pool)
    .await?;

    Ok(exists.is_some())
}
