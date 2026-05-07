use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboundRule {
    pub port: u16,
    pub address: IpAddr,
}

impl InboundRule {
    pub fn is_ipv4(&self) -> bool {
        self.address.is_ipv4()
    }
}

impl FromStr for InboundRule {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        // Support IPv6 bracket notation: port:[ipv6_addr]
        if let Some(bracket_start) = input.find('[') {
            let bracket_end = input
                .find(']')
                .ok_or_else(|| format!("Missing closing bracket in inbound rule: '{}'", input))?;

            let port = input[..bracket_start]
                .trim_end_matches(':')
                .parse::<u16>()
                .map_err(|_| format!("Invalid port in inbound rule: '{}'", input))?;
            let address = input[bracket_start + 1..bracket_end]
                .parse::<IpAddr>()
                .map_err(|_| format!("Invalid address in inbound rule: '{}'", input))?;

            Ok(InboundRule { port, address })
        } else {
            // IPv4 format: port:address
            let parts: Vec<&str> = input.splitn(2, ':').collect();
            if parts.len() != 2 {
                return Err(format!(
                    "Invalid inbound rule format: '{}'. Expected: 'port:address' or 'port:[ipv6_addr]'",
                    input
                ));
            }

            let port = parts[0]
                .parse::<u16>()
                .map_err(|_| format!("Invalid port: '{}'", parts[0]))?;
            let address = parts[1]
                .parse::<IpAddr>()
                .map_err(|_| format!("Invalid address: '{}'", parts[1]))?;

            Ok(InboundRule { port, address })
        }
    }
}

impl fmt::Display for InboundRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.address.is_ipv6() {
            write!(f, "{}:[{}]", self.port, self.address)
        } else {
            write!(f, "{}:{}", self.port, self.address)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InboundRuleList {
    pub rules: Vec<InboundRule>,
}

impl InboundRuleList {
    pub fn new(input: &str) -> Result<Self, String> {
        if input.is_empty() {
            Ok(Self { rules: vec![] })
        } else {
            let rules = input
                .split(',')
                .map(str::trim)
                .map(InboundRule::from_str)
                .collect::<Result<Vec<InboundRule>, _>>()?;
            Ok(Self { rules })
        }
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }
}

impl fmt::Display for InboundRuleList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.rules
                .iter()
                .map(|r| r.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_parse() {
        let rule = InboundRule::from_str("80:203.0.113.50").unwrap();
        assert_eq!(rule.port, 80);
        assert_eq!(rule.address, "203.0.113.50".parse::<IpAddr>().unwrap());
        assert!(rule.is_ipv4());
    }

    #[test]
    fn test_ipv6_parse() {
        let rule = InboundRule::from_str("443:[2001:db8::50]").unwrap();
        assert_eq!(rule.port, 443);
        assert_eq!(rule.address, "2001:db8::50".parse::<IpAddr>().unwrap());
        assert!(!rule.is_ipv4());
    }

    #[test]
    fn test_list_parse() {
        let list = InboundRuleList::new("443:[2001:db8::50], 22:[2001:db8::10]").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list.rules[0].port, 443);
        assert_eq!(list.rules[1].port, 22);
    }

    #[test]
    fn test_empty_list() {
        let list = InboundRuleList::new("").unwrap();
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_display_ipv6() {
        let rule = InboundRule::from_str("443:[2001:db8::50]").unwrap();
        assert_eq!(rule.to_string(), "443:[2001:db8::50]");
    }

    #[test]
    fn test_display_ipv4() {
        let rule = InboundRule::from_str("80:10.0.0.1").unwrap();
        assert_eq!(rule.to_string(), "80:10.0.0.1");
    }

    #[test]
    fn test_invalid_format() {
        assert!(InboundRule::from_str("noport").is_err());
        assert!(InboundRule::from_str("abc:1.2.3.4").is_err());
        assert!(InboundRule::from_str("80:not_an_ip").is_err());
    }
}
