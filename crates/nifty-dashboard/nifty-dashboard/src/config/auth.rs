use crate::errors::CliError;
use crate::middleware::auth::AuthenticationMethod;
use conf::Conf;
use serde::{Deserialize, Serialize};

const DEFAULT_TRUSTED_USER_HEADER: &str = "x-forwarded-user";
const DEFAULT_FORWARDED_FOR_HEADER: &str = "x-forwarded-for";

#[derive(Conf, Debug, Clone, Serialize, Deserialize)]
#[conf(serde)]
pub struct AuthConfig {
    /// Authentication method to use: forward_auth or username_password.
    #[arg(long = "auth-method", env = "AUTH_METHOD")]
    #[conf(default(AuthenticationMethod::UsernamePassword))]
    pub method: AuthenticationMethod,

    /// Header to read the authenticated user email from.
    #[arg(long = "auth-trusted-header-name", env = "AUTH_TRUSTED_HEADER_NAME")]
    #[conf(default(DEFAULT_TRUSTED_USER_HEADER.to_string()))]
    pub trusted_header_name: String,

    /// Only trust the header when the TCP peer IP matches this proxy.
    #[arg(long = "auth-trusted-proxy", env = "AUTH_TRUSTED_PROXY")]
    pub trusted_proxy: Option<std::net::IpAddr>,

    /// Enable trusting X-Forwarded-For (or custom) from a trusted proxy.
    #[arg(
        long = "auth-trusted-forwarded-for",
        env = "AUTH_TRUSTED_FORWARDED_FOR"
    )]
    pub trusted_forwarded_for: bool,

    /// Header to read client IP from when trusted-forwarded-for is enabled.
    #[arg(
        long = "auth-trusted-forwarded-for-name",
        env = "AUTH_TRUSTED_FORWARDED_FOR_NAME"
    )]
    #[conf(default(DEFAULT_FORWARDED_FOR_HEADER.to_string()))]
    pub trusted_forwarded_for_name: String,

    /// OIDC (OAuth2) issuer
    #[arg(long = "auth-oidc-issuer", env = "AUTH_OIDC_ISSUER")]
    pub oidc_issuer: Option<String>,
    /// OIDC (OAuth2) client id
    #[arg(long = "auth-oidc-client-id", env = "AUTH_OIDC_CLIENT_ID")]
    pub oidc_client_id: Option<String>,
    /// OIDC (OAuth2) client secret
    #[arg(long = "auth-oidc-client-secret", env = "AUTH_OIDC_CLIENT_SECRET")]
    pub oidc_client_secret: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            method: AuthenticationMethod::UsernamePassword,
            trusted_header_name: DEFAULT_TRUSTED_USER_HEADER.to_string(),
            trusted_proxy: None,
            trusted_forwarded_for: false,
            trusted_forwarded_for_name: DEFAULT_FORWARDED_FOR_HEADER.to_string(),
            oidc_issuer: None,
            oidc_client_id: None,
            oidc_client_secret: None,
        }
    }
}

impl AuthConfig {
    pub fn validate(&self) -> Result<(), CliError> {
        if matches!(self.method, AuthenticationMethod::ForwardAuth) && self.trusted_proxy.is_none()
        {
            return Err(CliError::InvalidArgs(
                "auth-trusted-proxy is required when auth-method=forward_auth".into(),
            ));
        }
        Ok(())
    }
}
