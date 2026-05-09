use strum::IntoEnumIterator;
use strum_macros::{Display, EnumIter, EnumString};

#[derive(Debug, Clone, PartialEq, EnumString, EnumIter, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum QosClass {
    Voice,
    Video,
    Besteffort,
    Bulk,
}

impl QosClass {
    pub fn new(input: &str) -> Result<Self, String> {
        input.to_lowercase().parse::<QosClass>().map_err(|_| {
            format!(
                "Invalid QoS class value: '{}'. Acceptable values are: {}",
                input,
                QosClass::variants().join(", ")
            )
        })
    }

    fn variants() -> Vec<String> {
        QosClass::iter()
            .map(|variant| variant.to_string())
            .collect()
    }

    /// Returns the nftables DSCP keyword for this QoS class.
    pub fn dscp_name(&self) -> &str {
        match self {
            QosClass::Voice => "ef",
            QosClass::Video => "af41",
            QosClass::Besteffort => "cs0",
            QosClass::Bulk => "cs1",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qos_class_from_string() {
        assert_eq!(QosClass::new("voice").unwrap(), QosClass::Voice);
        assert_eq!(QosClass::new("video").unwrap(), QosClass::Video);
        assert_eq!(QosClass::new("besteffort").unwrap(), QosClass::Besteffort);
        assert_eq!(QosClass::new("bulk").unwrap(), QosClass::Bulk);
    }

    #[test]
    fn test_qos_class_case_insensitive() {
        assert_eq!(QosClass::new("Voice").unwrap(), QosClass::Voice);
        assert_eq!(QosClass::new("VOICE").unwrap(), QosClass::Voice);
        assert_eq!(QosClass::new("VoIcE").unwrap(), QosClass::Voice);
    }

    #[test]
    fn test_qos_class_invalid() {
        let err = QosClass::new("invalid").unwrap_err();
        assert!(err.contains("Invalid QoS class"));
        assert!(err.contains("voice"));
        assert!(err.contains("bulk"));
    }

    #[test]
    fn test_qos_class_dscp_names() {
        assert_eq!(QosClass::Voice.dscp_name(), "ef");
        assert_eq!(QosClass::Video.dscp_name(), "af41");
        assert_eq!(QosClass::Besteffort.dscp_name(), "cs0");
        assert_eq!(QosClass::Bulk.dscp_name(), "cs1");
    }

    #[test]
    fn test_qos_class_display() {
        assert_eq!(QosClass::Voice.to_string(), "voice");
        assert_eq!(QosClass::Video.to_string(), "video");
        assert_eq!(QosClass::Besteffort.to_string(), "besteffort");
        assert_eq!(QosClass::Bulk.to_string(), "bulk");
    }
}
