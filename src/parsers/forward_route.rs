use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardRoute {
    pub incoming_port: u16,
    pub destination_ip: IpAddr,
    pub destination_port: u16,
}

impl ForwardRoute {
    pub fn is_ipv4(&self) -> bool {
        self.destination_ip.is_ipv4()
    }
}

impl FromStr for ForwardRoute {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        // Support IPv6 bracket notation: incoming_port:[ipv6_addr]:destination_port
        // IPv4 format remains: incoming_port:ipv4_addr:destination_port
        if let Some(bracket_start) = input.find('[') {
            let bracket_end = input
                .find(']')
                .ok_or_else(|| format!("Missing closing bracket in forward route: '{}'", input))?;

            let incoming_port = input[..bracket_start]
                .trim_end_matches(':')
                .parse::<u16>()
                .map_err(|_| format!("Invalid incoming port in: '{}'", input))?;
            let destination_ip = input[bracket_start + 1..bracket_end]
                .parse::<IpAddr>()
                .map_err(|_| format!("Invalid destination IP in: '{}'", input))?;
            let destination_port = input[bracket_end + 1..]
                .trim_start_matches(':')
                .parse::<u16>()
                .map_err(|_| format!("Invalid destination port in: '{}'", input))?;

            Ok(ForwardRoute {
                incoming_port,
                destination_ip,
                destination_port,
            })
        } else {
            let parts: Vec<&str> = input.split(':').collect();
            if parts.len() != 3 {
                return Err(format!("Invalid forward route format: '{}'. Expected format: 'incoming_port:destination_ip:destination_port' (use brackets for IPv6: 'port:[ipv6_addr]:port')", input));
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
}

impl fmt::Display for ForwardRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.destination_ip.is_ipv6() {
            write!(
                f,
                "{}:[{}]:{}",
                self.incoming_port, self.destination_ip, self.destination_port
            )
        } else {
            write!(
                f,
                "{}:{}:{}",
                self.incoming_port, self.destination_ip, self.destination_port
            )
        }
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

    #[test]
    fn test_forward_route_ipv6_bracket() {
        let input = "8080:[fd00::50]:80";
        let route = ForwardRoute::from_str(input).unwrap();
        assert_eq!(route.incoming_port, 8080);
        assert_eq!(route.destination_ip, "fd00::50".parse::<IpAddr>().unwrap());
        assert_eq!(route.destination_port, 80);
        assert!(!route.is_ipv4());
    }

    #[test]
    fn test_forward_route_ipv6_display() {
        let input = "8080:[fd00::50]:80";
        let route = ForwardRoute::from_str(input).unwrap();
        assert_eq!(route.to_string(), "8080:[fd00::50]:80");
    }

    #[test]
    fn test_forward_route_list_mixed() {
        let input = "8080:192.168.1.100:80, 8443:[fd00::50]:443";
        let route_list = ForwardRouteList::new(input).unwrap();
        assert_eq!(route_list.len(), 2);
        assert!(route_list.routes[0].is_ipv4());
        assert!(!route_list.routes[1].is_ipv4());
    }
}
