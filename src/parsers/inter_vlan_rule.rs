use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

/// An inter-VLAN allow rule.
///
/// 2-tuple format: `dest:port` — allow entire source VLAN to reach dest:port
/// 3-tuple format: `src:dest:port` — allow only src to reach dest:port
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterVlanRule {
    pub dest: IpAddr,
    pub port: u16,
    pub src: Option<IpAddr>,
}

impl InterVlanRule {
    pub fn dest_is_ipv4(&self) -> bool {
        self.dest.is_ipv4()
    }

    pub fn has_src(&self) -> bool {
        self.src.is_some()
    }

    pub fn src_is_ipv4(&self) -> bool {
        self.src.map_or(false, |s| s.is_ipv4())
    }
}

/// Parse an IPv6 bracket address starting at `input[start..]`.
/// Returns (IpAddr, end_index_after_bracket).
fn parse_bracketed_ipv6(input: &str, start: usize) -> Result<(IpAddr, usize), String> {
    if input.as_bytes().get(start) != Some(&b'[') {
        return Err(format!("Expected '[' at position {} in '{}'", start, input));
    }
    let bracket_end = input[start..]
        .find(']')
        .ok_or_else(|| format!("Missing closing bracket in '{}'", input))?
        + start;
    let addr = input[start + 1..bracket_end]
        .parse::<IpAddr>()
        .map_err(|_| format!("Invalid IPv6 address in '{}'", input))?;
    Ok((addr, bracket_end + 1))
}

impl FromStr for InterVlanRule {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();

        // Detect IPv6 by bracket presence
        if input.contains('[') {
            // Could be [dest]:port or [src]:[dest]:port
            let (first_addr, after_first) = parse_bracketed_ipv6(input, 0)?;

            // After first bracket group, expect ':'
            if input.as_bytes().get(after_first) != Some(&b':') {
                return Err(format!("Expected ':' after address in '{}'", input));
            }
            let rest = &input[after_first + 1..];

            if rest.starts_with('[') {
                // 3-tuple: [src]:[dest]:port
                let (second_addr, after_second) = parse_bracketed_ipv6(rest, 0)?;
                if rest.as_bytes().get(after_second) != Some(&b':') {
                    return Err(format!("Expected ':' after second address in '{}'", input));
                }
                let port = rest[after_second + 1..]
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid port in '{}'", input))?;
                Ok(InterVlanRule {
                    dest: second_addr,
                    port,
                    src: Some(first_addr),
                })
            } else {
                // 2-tuple: [dest]:port
                let port = rest
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid port in '{}'", input))?;
                Ok(InterVlanRule {
                    dest: first_addr,
                    port,
                    src: None,
                })
            }
        } else {
            // IPv4: dest:port or src:dest:port
            let parts: Vec<&str> = input.split(':').collect();
            match parts.len() {
                2 => {
                    // dest:port
                    let dest = parts[0]
                        .parse::<IpAddr>()
                        .map_err(|_| format!("Invalid destination address: '{}'", parts[0]))?;
                    let port = parts[1]
                        .parse::<u16>()
                        .map_err(|_| format!("Invalid port: '{}'", parts[1]))?;
                    Ok(InterVlanRule {
                        dest,
                        port,
                        src: None,
                    })
                }
                3 => {
                    // src:dest:port
                    let src = parts[0]
                        .parse::<IpAddr>()
                        .map_err(|_| format!("Invalid source address: '{}'", parts[0]))?;
                    let dest = parts[1]
                        .parse::<IpAddr>()
                        .map_err(|_| format!("Invalid destination address: '{}'", parts[1]))?;
                    let port = parts[2]
                        .parse::<u16>()
                        .map_err(|_| format!("Invalid port: '{}'", parts[2]))?;
                    Ok(InterVlanRule {
                        dest,
                        port,
                        src: Some(src),
                    })
                }
                _ => Err(format!(
                    "Invalid inter-VLAN rule format: '{}'. Expected 'dest:port' or 'src:dest:port'",
                    input
                )),
            }
        }
    }
}

impl fmt::Display for InterVlanRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.src {
            Some(src) => {
                if src.is_ipv6() {
                    write!(f, "[{}]:", src)?;
                } else {
                    write!(f, "{}:", src)?;
                }
            }
            None => {}
        }
        if self.dest.is_ipv6() {
            write!(f, "[{}]:{}", self.dest, self.port)
        } else {
            write!(f, "{}:{}", self.dest, self.port)
        }
    }
}

/// A keyed set of inter-VLAN allow rules, indexed by source VLAN ID.
#[derive(Debug, PartialEq, Eq)]
pub struct InterVlanRuleEntry {
    pub source_vlan_id: u16,
    pub source_interface: String,
    pub rules: Vec<InterVlanRule>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InterVlanRuleList {
    pub entries: Vec<InterVlanRuleEntry>,
}

impl InterVlanRuleList {
    pub fn new() -> Self {
        Self { entries: vec![] }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn add_entry(
        &mut self,
        source_vlan_id: u16,
        source_interface: String,
        input: &str,
    ) -> Result<(), String> {
        if input.is_empty() {
            return Ok(());
        }
        let rules = input
            .split(',')
            .map(str::trim)
            .map(InterVlanRule::from_str)
            .collect::<Result<Vec<InterVlanRule>, _>>()?;
        if !rules.is_empty() {
            self.entries.push(InterVlanRuleEntry {
                source_vlan_id,
                source_interface,
                rules,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_2tuple() {
        let rule = InterVlanRule::from_str("10.99.40.5:80").unwrap();
        assert_eq!(rule.dest, "10.99.40.5".parse::<IpAddr>().unwrap());
        assert_eq!(rule.port, 80);
        assert!(rule.src.is_none());
        assert!(rule.dest_is_ipv4());
    }

    #[test]
    fn test_ipv4_3tuple() {
        let rule = InterVlanRule::from_str("10.99.10.50:10.99.40.5:443").unwrap();
        assert_eq!(rule.src.unwrap(), "10.99.10.50".parse::<IpAddr>().unwrap());
        assert_eq!(rule.dest, "10.99.40.5".parse::<IpAddr>().unwrap());
        assert_eq!(rule.port, 443);
        assert!(rule.has_src());
        assert!(rule.src_is_ipv4());
    }

    #[test]
    fn test_ipv6_2tuple() {
        let rule = InterVlanRule::from_str("[fd00:40::5]:80").unwrap();
        assert_eq!(rule.dest, "fd00:40::5".parse::<IpAddr>().unwrap());
        assert_eq!(rule.port, 80);
        assert!(rule.src.is_none());
        assert!(!rule.dest_is_ipv4());
    }

    #[test]
    fn test_ipv6_3tuple() {
        let rule = InterVlanRule::from_str("[fd00:10::50]:[fd00:40::5]:443").unwrap();
        assert_eq!(
            rule.src.unwrap(),
            "fd00:10::50".parse::<IpAddr>().unwrap()
        );
        assert_eq!(rule.dest, "fd00:40::5".parse::<IpAddr>().unwrap());
        assert_eq!(rule.port, 443);
    }

    #[test]
    fn test_display_ipv4_2tuple() {
        let rule = InterVlanRule::from_str("10.99.40.5:80").unwrap();
        assert_eq!(rule.to_string(), "10.99.40.5:80");
    }

    #[test]
    fn test_display_ipv4_3tuple() {
        let rule = InterVlanRule::from_str("10.99.10.50:10.99.40.5:443").unwrap();
        assert_eq!(rule.to_string(), "10.99.10.50:10.99.40.5:443");
    }

    #[test]
    fn test_display_ipv6_2tuple() {
        let rule = InterVlanRule::from_str("[fd00:40::5]:80").unwrap();
        assert_eq!(rule.to_string(), "[fd00:40::5]:80");
    }

    #[test]
    fn test_display_ipv6_3tuple() {
        let rule = InterVlanRule::from_str("[fd00:10::50]:[fd00:40::5]:443").unwrap();
        assert_eq!(rule.to_string(), "[fd00:10::50]:[fd00:40::5]:443");
    }

    #[test]
    fn test_list_add_entry() {
        let mut list = InterVlanRuleList::new();
        list.add_entry(10, "trusted".to_string(), "10.99.40.5:80, 10.99.10.50:10.99.40.5:443")
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list.entries[0].source_vlan_id, 10);
        assert_eq!(list.entries[0].source_interface, "trusted");
        assert_eq!(list.entries[0].rules.len(), 2);
    }

    #[test]
    fn test_list_empty_input() {
        let mut list = InterVlanRuleList::new();
        list.add_entry(10, "trusted".to_string(), "").unwrap();
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_invalid_format() {
        assert!(InterVlanRule::from_str("noport").is_err());
        assert!(InterVlanRule::from_str("a:b:c:d").is_err());
        assert!(InterVlanRule::from_str("not_ip:80").is_err());
        assert!(InterVlanRule::from_str("10.0.0.1:notaport").is_err());
    }
}
