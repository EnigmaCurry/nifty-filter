use std::fmt;

#[derive(Clone, Debug)]
pub struct Lease {
    mac_address: String,
    ip_address: String,
    hostname: String,
    lease_time: String,
}

impl Lease {
    pub fn new(
        mac_address: String,
        ip_address: String,
        hostname: String,
        lease_time: String,
    ) -> Self {
        Lease {
            mac_address,
            ip_address,
            hostname,
            lease_time,
        }
    }
}

impl fmt::Display for Lease {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.mac_address, self.ip_address, self.hostname, self.lease_time
        )
    }
}

pub struct StaticLeases {
    leases: Vec<Lease>,
}

impl StaticLeases {
    pub fn new(data: &str) -> Result<Self, String> {
        let mut leases = Vec::new();

        for line in data.lines() {
            let fields: Vec<&str> = line.split(',').collect();

            if fields.len() == 4 {
                let mac_address = fields[0].to_string();
                let ip_address = fields[1].to_string();
                let hostname = fields[2].to_string();
                let lease_time = fields[3].to_string();

                leases.push(Lease::new(mac_address, ip_address, hostname, lease_time));
            } else {
                return Err("Invalid data format".to_string());
            }
        }

        Ok(StaticLeases { leases })
    }

    pub fn get_leases(&self) -> &Vec<Lease> {
        &self.leases
    }
}
