// src/model/identity_provider.rs
use crate::models::ids::IdentityProviderId;
#[allow(unused_imports)]
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use strum_macros::EnumString;

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct IdentityProvider {
    pub id: IdentityProviderId,
    pub name: String,
    pub display_name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum IdentityProviders {
    System,
    ForwardAuth,
    Oidc,
}
