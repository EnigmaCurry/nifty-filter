use strum::IntoEnumIterator;
use strum_macros::{Display, EnumIter, EnumString};

#[derive(Debug, Clone, EnumString, EnumIter, Display)]
#[strum(serialize_all = "lowercase")]
#[allow(dead_code)]
pub enum ChainPolicy {
    Accept,
    Drop,
    Reject,
    Continue,
    Return,
    Queue,
    Log,
}

impl ChainPolicy {
    pub fn new(input: &str) -> Result<Self, String> {
        input.to_lowercase().parse::<ChainPolicy>().map_err(|_| {
            format!(
                "Invalid chain policy value: '{}'. Acceptable values are: {}",
                input,
                ChainPolicy::variants().join(", ")
            )
        })
    }

    fn variants() -> Vec<String> {
        ChainPolicy::iter()
            .map(|variant| variant.to_string())
            .collect()
    }
}
