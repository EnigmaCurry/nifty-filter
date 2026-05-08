use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Type, Serialize, Deserialize, JsonSchema)]
#[sqlx(transparent)]
pub struct UserId(pub i64);

impl From<i64> for UserId {
    fn from(v: i64) -> Self {
        Self(v)
    }
}

impl UserId {
    #[allow(dead_code)]
    pub const fn new(v: i64) -> Self {
        Self(v)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Type, Serialize, Deserialize)]
#[sqlx(transparent)]
pub struct IdentityProviderId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Type, Serialize, Deserialize)]
#[sqlx(transparent)]
pub struct SignupMethodId(pub i64);

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Type, Serialize, Deserialize)]
#[sqlx(transparent)]
pub struct RoleId(pub i64);
