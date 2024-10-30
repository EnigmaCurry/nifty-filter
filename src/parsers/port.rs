use std::fmt;
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq)]
pub struct Port {
    number: u16,
}

impl Port {
    /// Creates a new `Port` if the input is a valid port number (1-65535).
    pub fn new(input: &str) -> Result<Self, String> {
        match input.parse::<u16>() {
            Ok(num) if num > 0 => Ok(Self { number: num }),
            _ => Err(format!("Invalid port number: {}", input)),
        }
    }

    /// Retrieve the port number as `u16`.
    #[allow(dead_code)]
    pub fn get(&self) -> u16 {
        self.number
    }
}

impl fmt::Display for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.number)
    }
}

impl FromStr for Port {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Port::new(input)
    }
}
