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

#[derive(Debug, PartialEq, Eq)]
pub struct PortList {
    ports: Vec<Port>,
}

impl PortList {
    /// Creates a new `PortList` from a comma-separated string of ports.
    pub fn new(input: &str) -> Result<Self, String> {
        if input.is_empty() {
            let ports = vec![];
            Ok(Self { ports })
        } else {
            let ports = input
                .split(',')
                .map(str::trim)
                .map(Port::from_str)
                .collect::<Result<Vec<Port>, _>>()
                .map_err(|e| e.to_string())?;
            Ok(Self { ports })
        }
    }

    /// Retrieve the list of ports as a `Vec<u16>`.
    #[allow(dead_code)]
    pub fn get_ports(&self) -> Vec<u16> {
        self.ports.iter().map(|port| port.get()).collect()
    }

    pub fn to_string(&self) -> String {
        self.ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl fmt::Display for PortList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ports_str = self
            .ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "{}", ports_str)
    }
}

impl From<String> for PortList {
    fn from(input: String) -> Self {
        PortList::new(&input).unwrap_or_else(|_| PortList { ports: vec![] })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_list_parsing() {
        let input = "22, 80, 443";
        let port_list = PortList::new(input).unwrap();
        assert_eq!(port_list.get_ports(), vec![22, 80, 443]);
    }

    #[test]
    fn test_port_list_invalid() {
        let input = "22, abc, 443";
        assert!(PortList::new(input).is_err());
    }
}
