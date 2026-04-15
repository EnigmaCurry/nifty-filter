use strum::IntoEnumIterator;
use strum_macros::{Display, EnumIter, EnumString};

#[derive(Debug, Clone, EnumString, EnumIter, Display)]
#[strum(serialize_all = "kebab-case")]
#[allow(dead_code)]
pub enum Icmpv6Type {
    DestinationUnreachable,
    PacketTooBig,
    TimeExceeded,
    ParameterProblem,
    EchoRequest,
    EchoReply,
    NdRouterSolicit,
    NdRouterAdvert,
    NdNeighborSolicit,
    NdNeighborAdvert,
}

impl Icmpv6Type {
    pub fn new(input: &str) -> Result<Self, String> {
        input.to_lowercase().parse::<Icmpv6Type>().map_err(|_| {
            format!(
                "Invalid ICMPv6 type value: '{}'. Acceptable values are: {}",
                input,
                Icmpv6Type::variants().join(", ")
            )
        })
    }

    fn variants() -> Vec<String> {
        Icmpv6Type::iter()
            .map(|variant| variant.to_string())
            .collect()
    }
}

impl Icmpv6Type {
    pub fn vec_to_string(vec: &[Icmpv6Type]) -> String {
        vec.iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
