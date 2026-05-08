use crate::models::ids::{RoleId, UserId};
use crate::models::user::User;
use sqlx::{FromRow, SqlitePool};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct UserWithRoles {
    pub user: User,
    pub roles: Vec<RoleId>,
    pub role_names: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, FromRow)]
struct RoleRow {
    pub id: RoleId,
    pub name: String,
}

#[allow(dead_code)]
pub async fn get_user_with_roles(
    pool: &SqlitePool,
    id: UserId,
) -> sqlx::Result<Option<UserWithRoles>> {
    // load user
    let user = match sqlx::query_as::<_, User>(r#"SELECT * FROM users WHERE id = ?1"#)
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
    {
        Some(u) => u,
        None => return Ok(None),
    };

    // load roles
    let role_rows: Vec<RoleRow> = sqlx::query_as::<_, RoleRow>(
        r#"
        SELECT r.id, r.name
        FROM [role] r
        JOIN user_roles ur ON ur.role_id = r.id
        WHERE ur.user_id = ?1
        "#,
    )
    .bind(id.0.to_string())
    .fetch_all(pool)
    .await?;

    let roles = role_rows.iter().map(|r| r.id).collect();
    let role_names = role_rows.into_iter().map(|r| r.name).collect();

    Ok(Some(UserWithRoles {
        user,
        roles,
        role_names,
    }))
}
