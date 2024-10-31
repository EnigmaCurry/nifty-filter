use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardRoute {
    pub incoming_port: u16,
    pub destination_ip: IpAddr,
    pub destination_port: u16,
}

impl FromStr for ForwardRoute {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = input.split(':').collect();
        if parts.len() != 3 {
            return Err(format!("Invalid forward route format: '{}'. Expected format: 'incoming_port:destination_ip:destination_port'", input));
        }

        let incoming_port = parts[0]
            .parse::<u16>()
            .map_err(|_| format!("Invalid incoming port: '{}'", parts[0]))?;
        let destination_ip = parts[1]
            .parse::<IpAddr>()
            .map_err(|_| format!("Invalid destination IP: '{}'", parts[1]))?;
        let destination_port = parts[2]
            .parse::<u16>()
            .map_err(|_| format!("Invalid destination port: '{}'", parts[2]))?;

        Ok(ForwardRoute {
            incoming_port,
            destination_ip,
            destination_port,
        })
    }
}

impl fmt::Display for ForwardRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.incoming_port, self.destination_ip, self.destination_port
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ForwardRouteList {
    pub routes: Vec<ForwardRoute>,
}

impl ForwardRouteList {
    /// Creates a new `ForwardRouteList` from a comma-separated string of forward routes.
    pub fn new(input: &str) -> Result<Self, String> {
        if input.is_empty() {
            let routes = vec![];
            Ok(Self { routes })
        } else {
            let routes = input
                .split(',')
                .map(str::trim)
                .map(ForwardRoute::from_str)
                .collect::<Result<Vec<ForwardRoute>, _>>()
                .map_err(|e| e.to_string())?;
            Ok(Self { routes })
        }
    }

    /// Retrieve the list of forward routes as a `Vec<ForwardRoute>`.
    #[allow(dead_code)]
    pub fn get_routes(&self) -> Vec<ForwardRoute> {
        self.routes.clone()
    }

    /// Get the length of the forward routes list.
    pub fn len(&self) -> usize {
        self.routes.len()
    }
}

impl fmt::Display for ForwardRouteList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.routes
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
    fn test_forward_route_parsing() {
        let input = "8080:192.168.1.100:80, 8443:192.168.1.101:443";
        let route_list = ForwardRouteList::new(input).unwrap();
        assert_eq!(route_list.get_routes().len(), 2);
        assert_eq!(route_list.get_routes()[0].incoming_port, 8080);
        assert_eq!(
            route_list.get_routes()[0].destination_ip,
            "192.168.1.100".parse::<IpAddr>().unwrap()
        );
        assert_eq!(route_list.get_routes()[0].destination_port, 80);
    }

    #[test]
    fn test_forward_route_invalid() {
        let input = "8080:192.168.1.100, 8443:192.168.1.101:443";
        assert!(ForwardRouteList::new(input).is_err());
    }

    #[test]
    fn test_to_string() {
        let input = "8080:192.168.1.100:80, 8443:192.168.1.101:443";
        let route_list = ForwardRouteList::new(input).unwrap();
        assert_eq!(
            route_list.to_string(),
            "8080:192.168.1.100:80, 8443:192.168.1.101:443"
        );
    }
}
