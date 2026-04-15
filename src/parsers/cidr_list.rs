use ipnetwork::IpNetwork;
use std::fmt;
use std::str::FromStr;

#[derive(Debug)]
pub struct CidrList {
    pub cidrs: Vec<IpNetwork>,
}

impl CidrList {
    pub fn new(input: &str) -> Result<Self, String> {
        if input.is_empty() {
            Ok(Self { cidrs: vec![] })
        } else {
            let cidrs = input
                .split(',')
                .map(str::trim)
                .map(|s| {
                    IpNetwork::from_str(s)
                        .map_err(|_| format!("Invalid CIDR range: '{}'", s))
                })
                .collect::<Result<Vec<IpNetwork>, _>>()?;
            Ok(Self { cidrs })
        }
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.cidrs.len()
    }
}

impl fmt::Display for CidrList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.cidrs
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cidr_list_ipv4() {
        let list = CidrList::new("0.0.0.0/0").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list.to_string(), "0.0.0.0/0");
    }

    #[test]
    fn test_cidr_list_ipv6() {
        let list = CidrList::new("::/0").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list.to_string(), "::/0");
    }

    #[test]
    fn test_cidr_list_multiple() {
        let list = CidrList::new("10.0.0.0/8, 172.16.0.0/12").unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_cidr_list_empty() {
        let list = CidrList::new("").unwrap();
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_cidr_list_invalid() {
        assert!(CidrList::new("not-a-cidr").is_err());
    }
}
