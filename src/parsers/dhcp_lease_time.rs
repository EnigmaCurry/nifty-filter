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
            DHCPLeaseTime::Finite(duration) => {
                let hours = duration.as_secs() / 3600;
                write!(f, "{}h", hours)
            }
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
/// assert_eq!(parse_dhcp_lease_time("12h").unwrap(), DHCPLeaseTime::Finite(Duration::from_secs(43200)));
/// assert_eq!(parse_dhcp_lease_time("infinite").unwrap(), DHCPLeaseTime::Infinite);
/// ```
pub fn parse_dhcp_lease_time(input: &str) -> Result<DHCPLeaseTime, String> {
    if input.eq_ignore_ascii_case("infinite") {
        return Ok(DHCPLeaseTime::Infinite);
    }

    let num_part: String = input.chars().take_while(|c| c.is_ascii_digit()).collect();
    let unit_part: String = input.chars().skip_while(|c| c.is_ascii_digit()).collect();

    let value: u64 = num_part
        .parse()
        .map_err(|_| format!("Invalid numeric value in lease time: {}", input))?;

    let seconds = match unit_part.as_str() {
        "m" => value * 60,
        "h" => value * 3600,
        "d" => value * 86400,
        _ => return Err(format!("Invalid unit in lease time: {}", unit_part)),
    };

    if seconds < 3600 {
        return Err("Lease time must be at least 1 hour".to_string());
    }

    Ok(DHCPLeaseTime::Finite(Duration::from_secs(seconds)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_valid_lease_times() {
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
        assert!(parse_dhcp_lease_time("30m").is_err()); // Less than 1 hour
        assert!(parse_dhcp_lease_time("30x").is_err()); // Invalid unit
        assert!(parse_dhcp_lease_time("1w").is_err()); // Invalid unit
        assert!(parse_dhcp_lease_time("abc").is_err()); // Invalid format
    }

    #[test]
    fn test_display() {
        assert_eq!(
            format!("{}", DHCPLeaseTime::Finite(Duration::from_secs(43200))),
            "12h"
        );
        assert_eq!(format!("{}", DHCPLeaseTime::Infinite), "infinite");
    }
}
