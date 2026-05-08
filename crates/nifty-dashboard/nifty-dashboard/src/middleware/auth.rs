use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationMethod {
    #[default]
    UsernamePassword,
    ForwardAuth,
    Oidc,
}

impl fmt::Display for AuthenticationMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AuthenticationMethod::UsernamePassword => "username_password",
            AuthenticationMethod::ForwardAuth => "forward_auth",
            AuthenticationMethod::Oidc => "oidc",
        };
        write!(f, "{s}")
    }
}

impl FromStr for AuthenticationMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "username_password" => Ok(AuthenticationMethod::UsernamePassword),
            "forward_auth" => Ok(AuthenticationMethod::ForwardAuth),
            "oidc" => Ok(AuthenticationMethod::Oidc),
            other => Err(format!(
                "invalid auth method '{other}', expected one of: username_password, forward_auth, oidc"
            )),
        }
    }
}
