use crate::model::ids::SignupMethodId;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct SignupMethod {
    pub id: SignupMethodId,
    pub code: String,
    pub description: String,
}
