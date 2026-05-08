use super::AppConfig;
use app_macros;
use app_macros::app_conf_root;
use conf::Conf;

// app_macros::app_conf_root macro will hard-code the app prefix for env vars:
app_conf_root! {
    pub struct ServeConfig {
        /// CLI/env overrides for the server config.
        #[conf(flatten, serde(flatten))]
        pub app: AppConfig,
    }
}
