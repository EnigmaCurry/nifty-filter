use std::fmt;
use std::time::Duration;

/// Represents a parsed DHCP lease time.
#[derive(Debug, PartialEq)]
pub enum DHCPLeaseTime {
    Infinite,
    Finite(Duration),
}

impl fmt::Display for DHCPLeaseTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DHCPLeaseTime::Infinite => write!(f, "infinite"),
            DHCPLeaseTime::Finite(duration) => write!(f, "{} seconds", duration.as_secs()),
        }
    }
}

/// Parses a DHCP lease time string into a `DHCPLeaseTime` enum.
///
/// # Examples
/// ```
/// use validators::dhcp::{parse_dhcp_lease_time, DHCPLeaseTime};
/// use std::time::Duration;
///
/// assert_eq!(parse_dhcp_lease_time("30m").unwrap(), DHCPLeaseTime::Finite(Duration::from_secs(1800)));
/// assert_eq!(parse_dhcp_lease_time("1d").unwrap(), DHCPLeaseTime::Finite(Duration::from_secs(86400)));
/// assert_eq!(parse_dhcp_lease_time("infinite").unwrap(), DHCPLeaseTime::Infinite);
/// ```
pub fn parse_dhcp_lease_time(input: &str) -> Result<DHCPLeaseTime, String> {
    if input.eq_ignore_ascii_case("infinite") {
        return Ok(DHCPLeaseTime::Infinite);
    }

    let num_part: String = input.chars().take_while(|c| c.is_digit(10)).collect();
    let unit_part: String = input.chars().skip_while(|c| c.is_digit(10)).collect();

    let value: u64 = num_part
        .parse()
        .map_err(|_| format!("Invalid numeric value in lease time: {}", input))?;

    match unit_part.as_str() {
        "m" => Ok(DHCPLeaseTime::Finite(Duration::from_secs(value * 60))),
        "h" => Ok(DHCPLeaseTime::Finite(Duration::from_secs(value * 3600))),
        "d" => Ok(DHCPLeaseTime::Finite(Duration::from_secs(value * 86400))),
        _ => Err(format!("Invalid unit in lease time: {}", unit_part)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_valid_lease_times() {
        assert_eq!(
            parse_dhcp_lease_time("30m").unwrap(),
            DHCPLeaseTime::Finite(Duration::from_secs(1800))
        );
        assert_eq!(
            parse_dhcp_lease_time("12h").unwrap(),
            DHCPLeaseTime::Finite(Duration::from_secs(43200))
        );
        assert_eq!(
            parse_dhcp_lease_time("1d").unwrap(),
            DHCPLeaseTime::Finite(Duration::from_secs(86400))
        );
        assert_eq!(
            parse_dhcp_lease_time("infinite").unwrap(),
            DHCPLeaseTime::Infinite
        );
    }

    #[test]
    fn test_invalid_lease_times() {
        assert!(parse_dhcp_lease_time("30x").is_err());
        assert!(parse_dhcp_lease_time("1w").is_err());
        assert!(parse_dhcp_lease_time("abc").is_err());
    }
}
