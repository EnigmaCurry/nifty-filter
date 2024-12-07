use regex::Regex;
use std::fmt;

#[derive(Debug, Clone)]
pub struct Domain {
    name: String,
}

impl Domain {
    pub fn new(name: &str) -> Result<Self, String> {
        let domain_regex =
            Regex::new(r"(?i)^(?:[a-z0-9](?:[a-z0-9\-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$")
                .map_err(|_| "Failed to compile regex".to_string())?;
        if domain_regex.is_match(name) {
            Ok(Self {
                name: name.to_string(),
            })
        } else {
            Err(format!("'{}' is not a valid domain name", name))
        }
    }
}

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_domain() {
        let domain = Domain::new("example.com");
        assert!(domain.is_ok());
    }

    #[test]
    fn test_invalid_domain() {
        let domain = Domain::new("-invalid.com");
        assert!(domain.is_err());
        let domain = Domain::new("invalid⛵️.com");
        assert!(domain.is_err());
    }
}
