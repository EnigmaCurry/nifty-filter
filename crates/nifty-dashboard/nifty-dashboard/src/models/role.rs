use crate::models::ids::RoleId;
use sqlx::FromRow;
use strum_macros::AsRefStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum SystemRole {
    #[allow(dead_code)]
    User,
    Admin,
}

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct Role {
    pub id: RoleId,
    pub name: String,
    pub description: String,
}
