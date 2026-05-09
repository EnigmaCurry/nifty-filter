use std::env;

use ipnetwork::IpNetwork;

use crate::parsers::cidr_list::CidrList;
use crate::parsers::qos_class::QosClass;

/// A QoS override: a set of CIDRs that should be marked with a specific DSCP class,
/// split into IPv4 and IPv6 for separate nftables rule rendering.
pub struct QosOverride {
    pub class: QosClass,
    pub cidrs_ipv4: Vec<String>,
    pub cidrs_ipv6: Vec<String>,
}

impl QosOverride {
    pub fn from_cidr_list(class: QosClass, list: &CidrList) -> Self {
        let mut cidrs_ipv4 = Vec::new();
        let mut cidrs_ipv6 = Vec::new();
        for cidr in &list.cidrs {
            match cidr {
                IpNetwork::V4(_) => cidrs_ipv4.push(cidr.to_string()),
                IpNetwork::V6(_) => cidrs_ipv6.push(cidr.to_string()),
            }
        }
        Self {
            class,
            cidrs_ipv4,
            cidrs_ipv6,
        }
    }
}

/// Parsed QoS configuration for the tc/CAKE subcommand.
#[derive(Debug)]
pub struct QosConfig {
    pub upload_kbit: u32,
    pub download_kbit: u32,
}

impl QosConfig {
    /// Parse QoS speed configuration from environment variables.
    /// Returns Ok(None) when QoS is not configured (neither speed var set).
    /// Returns Ok(Some(config)) when both speeds are set.
    /// Returns Err when only one speed is set or values are invalid.
    pub fn from_env() -> Result<Option<Self>, Vec<String>> {
        let mut errors = Vec::new();

        let upload = env::var("WAN_QOS_UPLOAD_MBPS").ok();
        let download = env::var("WAN_QOS_DOWNLOAD_MBPS").ok();

        match (&upload, &download) {
            (None, None) => return Ok(None),
            (Some(_), None) => {
                errors.push(
                    "WAN_QOS_UPLOAD_MBPS is set but WAN_QOS_DOWNLOAD_MBPS is missing. Both are required for QoS."
                        .to_string(),
                );
                return Err(errors);
            }
            (None, Some(_)) => {
                errors.push(
                    "WAN_QOS_DOWNLOAD_MBPS is set but WAN_QOS_UPLOAD_MBPS is missing. Both are required for QoS."
                        .to_string(),
                );
                return Err(errors);
            }
            (Some(_), Some(_)) => {}
        }

        let upload_mbps: u32 = upload.unwrap().parse().unwrap_or_else(|_| {
            errors.push("WAN_QOS_UPLOAD_MBPS must be a positive integer.".to_string());
            0
        });
        let download_mbps: u32 = download.unwrap().parse().unwrap_or_else(|_| {
            errors.push("WAN_QOS_DOWNLOAD_MBPS must be a positive integer.".to_string());
            0
        });

        if upload_mbps == 0 && errors.is_empty() {
            errors.push("WAN_QOS_UPLOAD_MBPS must be greater than 0.".to_string());
        }
        if download_mbps == 0 && errors.is_empty() {
            errors.push("WAN_QOS_DOWNLOAD_MBPS must be greater than 0.".to_string());
        }

        let shave: u8 = env::var("WAN_QOS_SHAVE_PERCENT")
            .unwrap_or_else(|_| "10".to_string())
            .parse()
            .unwrap_or_else(|_| {
                errors.push(
                    "WAN_QOS_SHAVE_PERCENT must be an integer between 0 and 99.".to_string(),
                );
                10
            });
        if shave >= 100 {
            errors.push("WAN_QOS_SHAVE_PERCENT must be less than 100.".to_string());
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let factor = (100 - shave as u32) as u64;
        let upload_kbit = (upload_mbps as u64 * 1000 * factor / 100) as u32;
        let download_kbit = (download_mbps as u64 * 1000 * factor / 100) as u32;

        Ok(Some(QosConfig {
            upload_kbit,
            download_kbit,
        }))
    }
}

/// Parse QoS override CIDRs from environment variables.
pub fn parse_qos_overrides(errors: &mut Vec<String>) -> Vec<QosOverride> {
    let classes = [
        ("QOS_OVERRIDE_VOICE", QosClass::Voice),
        ("QOS_OVERRIDE_VIDEO", QosClass::Video),
        ("QOS_OVERRIDE_BESTEFFORT", QosClass::Besteffort),
        ("QOS_OVERRIDE_BULK", QosClass::Bulk),
    ];

    let mut overrides = Vec::new();
    for (var_name, class) in classes {
        if let Ok(val) = env::var(var_name) {
            if !val.is_empty() {
                match CidrList::new(&val) {
                    Ok(list) => {
                        let ovr = QosOverride::from_cidr_list(class, &list);
                        if !ovr.cidrs_ipv4.is_empty() || !ovr.cidrs_ipv6.is_empty() {
                            overrides.push(ovr);
                        }
                    }
                    Err(e) => {
                        errors.push(format!("{}: {}", var_name, e));
                    }
                }
            }
        }
    }
    overrides
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::ENV_LOCK;

    fn clear_qos_env() {
        for key in [
            "WAN_QOS_UPLOAD_MBPS",
            "WAN_QOS_DOWNLOAD_MBPS",
            "WAN_QOS_SHAVE_PERCENT",
            "QOS_OVERRIDE_VOICE",
            "QOS_OVERRIDE_VIDEO",
            "QOS_OVERRIDE_BESTEFFORT",
            "QOS_OVERRIDE_BULK",
        ] {
            env::remove_var(key);
        }
    }

    #[test]
    fn test_qos_disabled_when_no_speeds() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_qos_env();
        assert!(QosConfig::from_env().unwrap().is_none());
    }

    #[test]
    fn test_qos_one_speed_missing_upload() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_qos_env();
        env::set_var("WAN_QOS_DOWNLOAD_MBPS", "300");
        let err = QosConfig::from_env().unwrap_err();
        assert!(err.iter().any(|e| e.contains("WAN_QOS_UPLOAD_MBPS")));
        clear_qos_env();
    }

    #[test]
    fn test_qos_one_speed_missing_download() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_qos_env();
        env::set_var("WAN_QOS_UPLOAD_MBPS", "20");
        let err = QosConfig::from_env().unwrap_err();
        assert!(err.iter().any(|e| e.contains("WAN_QOS_DOWNLOAD_MBPS")));
        clear_qos_env();
    }

    #[test]
    fn test_qos_shave_calculation() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_qos_env();
        env::set_var("WAN_QOS_UPLOAD_MBPS", "100");
        env::set_var("WAN_QOS_DOWNLOAD_MBPS", "300");
        env::set_var("WAN_QOS_SHAVE_PERCENT", "15");

        let config = QosConfig::from_env().unwrap().unwrap();
        assert_eq!(config.upload_kbit, 85000); // 100 * 1000 * 85 / 100
        assert_eq!(config.download_kbit, 255000); // 300 * 1000 * 85 / 100
        clear_qos_env();
    }

    #[test]
    fn test_qos_default_shave() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_qos_env();
        env::set_var("WAN_QOS_UPLOAD_MBPS", "20");
        env::set_var("WAN_QOS_DOWNLOAD_MBPS", "300");

        let config = QosConfig::from_env().unwrap().unwrap();
        assert_eq!(config.upload_kbit, 18000); // 20 * 1000 * 90 / 100
        assert_eq!(config.download_kbit, 270000); // 300 * 1000 * 90 / 100
        clear_qos_env();
    }

    #[test]
    fn test_qos_shave_100_rejected() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_qos_env();
        env::set_var("WAN_QOS_UPLOAD_MBPS", "20");
        env::set_var("WAN_QOS_DOWNLOAD_MBPS", "300");
        env::set_var("WAN_QOS_SHAVE_PERCENT", "100");

        let err = QosConfig::from_env().unwrap_err();
        assert!(err.iter().any(|e| e.contains("less than 100")));
        clear_qos_env();
    }

    #[test]
    fn test_qos_zero_speed_rejected() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_qos_env();
        env::set_var("WAN_QOS_UPLOAD_MBPS", "0");
        env::set_var("WAN_QOS_DOWNLOAD_MBPS", "300");

        let err = QosConfig::from_env().unwrap_err();
        assert!(err.iter().any(|e| e.contains("greater than 0")));
        clear_qos_env();
    }

    #[test]
    fn test_qos_overrides_ipv4_ipv6_split() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_qos_env();
        env::set_var("QOS_OVERRIDE_VOICE", "10.0.10.50,fd00:10::50/128");

        let mut errors = Vec::new();
        let overrides = parse_qos_overrides(&mut errors);
        assert!(errors.is_empty());
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].class, QosClass::Voice);
        assert_eq!(overrides[0].cidrs_ipv4, vec!["10.0.10.50/32"]);
        assert_eq!(overrides[0].cidrs_ipv6, vec!["fd00:10::50/128"]);
        clear_qos_env();
    }

    #[test]
    fn test_qos_overrides_invalid_cidr() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_qos_env();
        env::set_var("QOS_OVERRIDE_BULK", "not-a-cidr");

        let mut errors = Vec::new();
        parse_qos_overrides(&mut errors);
        assert!(!errors.is_empty());
        assert!(errors[0].contains("QOS_OVERRIDE_BULK"));
        clear_qos_env();
    }
}
