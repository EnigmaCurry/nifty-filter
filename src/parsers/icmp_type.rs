use std::env;
use std::str::FromStr;
use strum::IntoEnumIterator;
use strum::ParseError;
use strum_macros::{Display, EnumIter, EnumString};

#[derive(Debug, Clone, EnumString, EnumIter, Display)]
#[strum(serialize_all = "kebab-case")]
#[allow(dead_code)]
pub enum IcmpType {
    EchoReply,
    DestinationUnreachable,
    SourceQuench,
    Redirect,
    EchoRequest,
    TimeExceeded,
    ParameterProblem,
    TimestampRequest,
    TimestampReply,
    InformationRequest,
    InformationReply,
    AddressMaskRequest,
    AddressMaskReply,
}

impl IcmpType {
    pub fn new(input: &str) -> Result<Self, String> {
        input.to_lowercase().parse::<IcmpType>().map_err(|_| {
            format!(
                "Invalid ICMP type value: '{}'. Acceptable values are: {}",
                input,
                IcmpType::variants().join(", ")
            )
        })
    }

    fn variants() -> Vec<String> {
        IcmpType::iter()
            .map(|variant| variant.to_string())
            .collect()
    }
}

impl IcmpType {
    pub fn vec_to_string(vec: &Vec<IcmpType>) -> String {
        vec.iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
