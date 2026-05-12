use ipnetwork::IpNetwork;

use crate::parsers::cidr_list::CidrList;
pub use crate::parsers::qos_class::QosClass;
use crate::hcl_config::QosHclConfig;

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

/// Per-VLAN bandwidth limit for HTB class shaping.
pub struct QosVlanBandwidth {
    pub vlan_id: u16,
    pub upload_kbit: u32,
}

/// Parsed QoS configuration for the tc/CAKE subcommand.
#[derive(Debug)]
pub struct QosConfig {
    pub upload_kbit: u32,
    pub download_kbit: u32,
}

impl QosConfig {
    /// Parse QoS configuration from an HCL config struct.
    pub fn from_hcl(qos: &QosHclConfig) -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();

        if qos.upload_mbps == 0 {
            errors.push("qos.upload_mbps must be greater than 0.".to_string());
        }
        if qos.download_mbps == 0 {
            errors.push("qos.download_mbps must be greater than 0.".to_string());
        }
        if qos.shave_percent >= 100 {
            errors.push("qos.shave_percent must be less than 100.".to_string());
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let factor = (100 - qos.shave_percent as u32) as u64;
        let upload_kbit = (qos.upload_mbps as u64 * 1000 * factor / 100) as u32;
        let download_kbit = (qos.download_mbps as u64 * 1000 * factor / 100) as u32;

        Ok(QosConfig {
            upload_kbit,
            download_kbit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_qos(upload: u32, download: u32, shave: u8) -> QosHclConfig {
        QosHclConfig {
            upload_mbps: upload,
            download_mbps: download,
            shave_percent: shave,
            overrides: None,
        }
    }

    #[test]
    fn test_qos_basic() {
        let config = QosConfig::from_hcl(&make_qos(20, 300, 10)).unwrap();
        assert_eq!(config.upload_kbit, 18000);   // 20 * 1000 * 90 / 100
        assert_eq!(config.download_kbit, 270000); // 300 * 1000 * 90 / 100
    }

    #[test]
    fn test_qos_custom_shave() {
        let config = QosConfig::from_hcl(&make_qos(100, 300, 15)).unwrap();
        assert_eq!(config.upload_kbit, 85000);   // 100 * 1000 * 85 / 100
        assert_eq!(config.download_kbit, 255000); // 300 * 1000 * 85 / 100
    }

    #[test]
    fn test_qos_zero_upload_rejected() {
        let err = QosConfig::from_hcl(&make_qos(0, 300, 10)).unwrap_err();
        assert!(err.iter().any(|e| e.contains("upload_mbps")));
    }

    #[test]
    fn test_qos_zero_download_rejected() {
        let err = QosConfig::from_hcl(&make_qos(20, 0, 10)).unwrap_err();
        assert!(err.iter().any(|e| e.contains("download_mbps")));
    }

    #[test]
    fn test_qos_shave_100_rejected() {
        let err = QosConfig::from_hcl(&make_qos(20, 300, 100)).unwrap_err();
        assert!(err.iter().any(|e| e.contains("less than 100")));
    }

    #[test]
    fn test_qos_overrides_split() {
        let list = CidrList::new("10.0.10.50,fd00:10::50/128").unwrap();
        let ovr = QosOverride::from_cidr_list(QosClass::Voice, &list);
        assert_eq!(ovr.cidrs_ipv4, vec!["10.0.10.50/32"]);
        assert_eq!(ovr.cidrs_ipv6, vec!["fd00:10::50/128"]);
    }

    #[test]
    fn test_qos_overrides_invalid_cidr() {
        assert!(CidrList::new("not-a-cidr").is_err());
    }
}
