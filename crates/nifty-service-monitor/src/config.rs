use serde::Deserialize;

#[derive(Deserialize)]
pub struct ApiResponse {
    pub error: Option<String>,
    pub data: Option<ApiData>,
}

#[derive(Deserialize)]
pub struct ApiData {
    pub services: ServicesConfig,
}

#[derive(Deserialize, Default)]
pub struct ServicesConfig {
    pub technitium: Option<TechnitiumConfig>,
}

#[derive(Deserialize)]
pub struct TechnitiumConfig {
    pub admin_password: Option<String>,
    pub domain: Option<String>,
    pub address: Option<String>,
}
