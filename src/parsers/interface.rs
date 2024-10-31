use regex::Regex;
use std::fmt;

pub struct Interface {
    name: String,
}

impl Interface {
    pub fn new(input: &str) -> Result<Self, String> {
        // Define the regex for a valid nftables interface name
        let valid_name_regex = Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9._-]{0,14}$").unwrap();

        if input.is_empty() {
            return Err("Interface name cannot be empty.".to_string());
        }

        if valid_name_regex.is_match(input) {
            Ok(Self {
                name: input.to_string(),
            })
        } else {
            Err(format!("Invalid interface name: '{}'", input))
        }
    }
}
impl fmt::Display for Interface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}
