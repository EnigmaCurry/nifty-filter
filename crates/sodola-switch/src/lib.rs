use md5::{Digest, Md5};
use serde::Serialize;
use std::fmt;
use std::io;
use std::io::Read;
use std::path::Path;

/// Client for communicating with a Sodola SL-SWTGW218AS managed switch.
///
/// The switch exposes a CGI-based web interface with cookie authentication.
/// Auth token is `md5(username + password)`, sent as the `admin` cookie.
///
/// Note: The switch firmware requires HTTP header names with standard casing
/// (e.g. `Cookie:` not `cookie:`). This is why we use ureq 2 (not 3 or reqwest).
pub struct SodolaClient {
    base_url: String,
    agent: ureq::Agent,
    auth_cookie: Option<String>,
}

impl SodolaClient {
    /// Create a new client pointing at the switch's HTTP interface.
    ///
    /// `base_url` should be like `http://192.168.2.1` (no trailing slash).
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            agent: ureq::Agent::new(),
            auth_cookie: None,
        }
    }

    /// Compute the auth cookie from username and password.
    /// The switch uses `md5(username + password)` as the session token.
    pub fn auth_token(username: &str, password: &str) -> String {
        let mut hasher = Md5::new();
        hasher.update(format!("{}{}", username, password));
        format!("{:x}", hasher.finalize())
    }

    /// Log in with username and password.
    /// Computes the MD5 token and POSTs to `/login.cgi` to establish the session.
    pub fn login(&mut self, username: &str, password: &str) -> Result<(), SodolaError> {
        let token = Self::auth_token(username, password);
        let url = format!("{}/login.cgi", self.base_url);

        let resp = self
            .agent
            .post(&url)
            .set("Cookie", &format!("admin={}", token))
            .send_form(&[
                ("username", username),
                ("password", password),
                ("Response", &token),
                ("language", "EN"),
            ])
            .map_err(|e| SodolaError::Http(e.to_string()))?;

        let body = resp.into_string().map_err(|e| SodolaError::Http(e.to_string()))?;

        if body.contains("location.replace(\"/login.cgi\")") {
            return Err(SodolaError::Http(
                "login rejected — check username/password".to_string(),
            ));
        }

        self.auth_cookie = Some(token);
        Ok(())
    }

    /// Log out, invalidating the session on the switch.
    pub fn logout(&mut self) -> Result<(), SodolaError> {
        let cookie = self.cookie_value()?;
        let url = format!("{}/logout.cgi", self.base_url);
        self.agent
            .get(&url)
            .set("Cookie", &cookie)
            .call()
            .map_err(|e| SodolaError::Http(e.to_string()))?;
        self.auth_cookie = None;
        Ok(())
    }

    /// Set the admin authentication cookie value directly (pre-computed token).
    pub fn set_auth_cookie(&mut self, cookie: &str) {
        self.auth_cookie = Some(cookie.to_string());
    }

    fn cookie_value(&self) -> Result<String, SodolaError> {
        let cookie = self
            .auth_cookie
            .as_deref()
            .ok_or(SodolaError::NotAuthenticated)?;
        Ok(format!("admin={}", cookie))
    }

    fn check_auth_redirect(body: &str) -> Result<(), SodolaError> {
        if body.contains("location.replace(\"/login.cgi\")") {
            return Err(SodolaError::SessionExpired);
        }
        Ok(())
    }

    fn get_page(&self, path: &str) -> Result<String, SodolaError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .agent
            .get(&url)
            .set("Cookie", &self.cookie_value()?)
            .call()
            .map_err(|e| SodolaError::Http(e.to_string()))?;
        let body = resp.into_string().map_err(|e| SodolaError::Http(e.to_string()))?;
        Self::check_auth_redirect(&body)?;
        Ok(body)
    }

    fn post_form(&self, path: &str, params: &[(&str, &str)]) -> Result<String, SodolaError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .agent
            .post(&url)
            .set("Cookie", &self.cookie_value()?)
            .send_form(params)
            .map_err(|e| SodolaError::Http(e.to_string()))?;
        let body = resp.into_string().map_err(|e| SodolaError::Http(e.to_string()))?;
        Self::check_auth_redirect(&body)?;
        Ok(body)
    }

    pub fn info(&self) -> Result<SwitchInfo, SodolaError> {
        let html = self.get_page("/info.cgi")?;
        SwitchInfo::parse(&html)
    }

    pub fn port_status(&self) -> Result<Vec<PortStatus>, SodolaError> {
        let html = self.get_page("/panel.cgi")?;
        Ok(PortStatus::parse_panel(&html))
    }

    pub fn port_stats(&self) -> Result<Vec<PortStats>, SodolaError> {
        let html = self.get_page("/port.cgi?page=stats")?;
        PortStats::parse_table(&html)
    }

    pub fn vlans(&self) -> Result<Vec<VlanEntry>, SodolaError> {
        let html = self.get_page("/vlan.cgi?page=static")?;
        Ok(VlanEntry::parse_table(&html))
    }

    pub fn pvid(&self) -> Result<Vec<PortVlanSetting>, SodolaError> {
        let html = self.get_page("/vlan.cgi?page=port_based")?;
        PortVlanSetting::parse_table(&html)
    }

    pub fn set_pvid(
        &self,
        ports: &[u8],
        pvid: u16,
        frame_type: AcceptedFrameType,
    ) -> Result<(), SodolaError> {
        let ft_val = match frame_type {
            AcceptedFrameType::All => "0",
            AcceptedFrameType::TagOnly => "1",
            AcceptedFrameType::UntagOnly => "2",
        };
        // Build owned strings for port values
        let port_strs: Vec<String> = ports.iter().map(|p| (p - 1).to_string()).collect();
        let pvid_str = pvid.to_string();
        let mut params: Vec<(&str, &str)> = Vec::new();
        for ps in &port_strs {
            params.push(("ports", ps));
        }
        params.push(("pvid", &pvid_str));
        params.push(("vlan_accept_frame_type", ft_val));
        self.post_form("/vlan.cgi?page=port_based", &params)?;
        Ok(())
    }

    pub fn set_vlan(
        &self,
        vid: u16,
        name: &str,
        port_modes: &[VlanPortMode; 9],
    ) -> Result<(), SodolaError> {
        let vid_str = vid.to_string();
        let mode_strs: Vec<String> = port_modes
            .iter()
            .map(|m| match m {
                VlanPortMode::Untagged => "0".to_string(),
                VlanPortMode::Tagged => "1".to_string(),
                VlanPortMode::NotMember => "2".to_string(),
            })
            .collect();
        let port_keys: Vec<String> = (0..9).map(|i| format!("vlanPort_{}", i)).collect();
        let mut params: Vec<(&str, &str)> = Vec::new();
        params.push(("vid", &vid_str));
        params.push(("name", name));
        for (key, val) in port_keys.iter().zip(mode_strs.iter()) {
            params.push((key, val));
        }
        self.post_form("/vlan.cgi?page=static", &params)?;
        Ok(())
    }

    /// Factory reset the switch (restores defaults and reboots, ~15 seconds).
    pub fn factory_reset(&self) -> Result<(), SodolaError> {
        self.post_form("/reset.cgi", &[("cmd", "factory_default")])?;
        Ok(())
    }

    /// Reboot the switch (~15 seconds).
    pub fn reboot(&self) -> Result<(), SodolaError> {
        self.post_form("/reboot.cgi", &[("cmd", "reboot")])?;
        Ok(())
    }

    /// Save running configuration to flash ROM.
    pub fn save(&self) -> Result<(), SodolaError> {
        self.post_form("/save.cgi", &[("cmd", "save")])?;
        Ok(())
    }

    pub fn delete_vlans(&self, vids: &[u16]) -> Result<(), SodolaError> {
        let keys: Vec<String> = vids.iter().map(|v| format!("remove_{}", v)).collect();
        let mut params: Vec<(&str, &str)> = Vec::new();
        for key in &keys {
            params.push((key, "on"));
        }
        params.push(("Delete", "    Delete    "));
        self.post_form("/vlan.cgi?page=getRmvVlanEntry", &params)?;
        Ok(())
    }

    pub fn backup(&self) -> Result<Vec<u8>, SodolaError> {
        let url = format!("{}/config_back.cgi?cmd=conf_backup", self.base_url);
        let resp = self
            .agent
            .get(&url)
            .set("Cookie", &self.cookie_value()?)
            .set("Referer", &format!("{}/config_back.cgi", self.base_url))
            .call()
            .map_err(|e| SodolaError::Http(e.to_string()))?;
        let mut buf = Vec::new();
        resp.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| SodolaError::Http(e.to_string()))?;
        Ok(buf)
    }

    /// Restore a configuration backup from raw bytes (requires reboot to take effect).
    pub fn restore(&self, data: &[u8]) -> Result<(), SodolaError> {
        let url = format!("{}/config_back.cgi?cmd=conf_restore", self.base_url);
        let boundary = "----sodola-switch-boundary";
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"submitFile\"; filename=\"switch_cfg.bin\"\r\n");
        body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(format!("\r\n--{}--\r\n", boundary).as_bytes());

        let resp = self
            .agent
            .post(&url)
            .set("Cookie", &self.cookie_value()?)
            .set("Content-Type", &format!("multipart/form-data; boundary={}", boundary))
            .send_bytes(&body)
            .map_err(|e| SodolaError::Http(e.to_string()))?;

        let body_str = resp.into_string().map_err(|e| SodolaError::Http(e.to_string()))?;
        if body_str.contains("Successfully") {
            Ok(())
        } else {
            Err(SodolaError::Http("restore failed — unexpected response".to_string()))
        }
    }

    /// Restore a configuration backup from a file (requires reboot to take effect).
    pub fn restore_from_file(&self, path: &Path) -> Result<(), SodolaError> {
        let data = std::fs::read(path).map_err(SodolaError::Io)?;
        self.restore(&data)
    }

    pub fn backup_to_file(&self, path: &Path) -> Result<usize, SodolaError> {
        let data = self.backup()?;
        std::fs::write(path, &data).map_err(SodolaError::Io)?;
        Ok(data.len())
    }
}

// --- Data types and parsers (unchanged) ---

#[derive(Debug, Clone, Serialize)]
pub struct SwitchInfo {
    pub device_type: String,
    pub mac_address: String,
    pub ip_address: String,
    pub netmask: String,
    pub gateway: String,
    pub firmware_version: String,
    pub firmware_date: String,
    pub hardware_version: String,
}

impl SwitchInfo {
    fn parse(html: &str) -> Result<SwitchInfo, SodolaError> {
        let field = |label: &str| -> Result<String, SodolaError> {
            let label_pos = html
                .find(label)
                .ok_or_else(|| SodolaError::Parse(format!("field '{}' not found", label)))?;
            let after_label = &html[label_pos..];
            let td_start = after_label
                .find("<td")
                .ok_or_else(|| SodolaError::Parse(format!("no <td> after '{}'", label)))?;
            let content_start = after_label[td_start..]
                .find('>')
                .ok_or_else(|| SodolaError::Parse(format!("malformed <td> for '{}'", label)))?;
            let content = &after_label[td_start + content_start + 1..];
            let end = content
                .find("</td")
                .ok_or_else(|| SodolaError::Parse(format!("no </td> for '{}'", label)))?;
            Ok(content[..end].trim().to_string())
        };
        Ok(SwitchInfo {
            device_type: field("Device Model")?,
            mac_address: field("MAC Address")?,
            ip_address: field("IP Address")?,
            netmask: field("Netmask")?,
            gateway: field("Gateway")?,
            firmware_version: field("Firmware Version")?,
            firmware_date: field("Firmware Date")?,
            hardware_version: field("Hardware Version")?,
        })
    }
}

impl fmt::Display for SwitchInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Device Model:     {}", self.device_type)?;
        writeln!(f, "MAC Address:      {}", self.mac_address)?;
        writeln!(f, "IP Address:       {}", self.ip_address)?;
        writeln!(f, "Netmask:          {}", self.netmask)?;
        writeln!(f, "Gateway:          {}", self.gateway)?;
        writeln!(f, "Firmware Version: {}", self.firmware_version)?;
        writeln!(f, "Firmware Date:    {}", self.firmware_date)?;
        write!(f, "Hardware Version: {}", self.hardware_version)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PortStatus {
    pub port: u8,
    pub port_type: PortType,
    pub link_up: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum PortType { Rj45, Fiber }

impl PortStatus {
    fn parse_panel(html: &str) -> Vec<PortStatus> {
        let mut ports = Vec::new();
        let mut port_num: u8 = 1;
        let mut search_from = 0;
        while let Some(pos) = html[search_from..].find("/RJ45_up_") {
            let abs_pos = search_from + pos;
            if let Some(c) = html[abs_pos + 9..].chars().next() {
                ports.push(PortStatus { port: port_num, port_type: PortType::Rj45, link_up: c == '1' });
                port_num += 1;
            }
            search_from = abs_pos + 10;
        }
        search_from = 0;
        while let Some(pos) = html[search_from..].find("/Fiber_up_") {
            let abs_pos = search_from + pos;
            if let Some(c) = html[abs_pos + 10..].chars().next() {
                ports.push(PortStatus { port: port_num, port_type: PortType::Fiber, link_up: c == '1' });
                port_num += 1;
            }
            search_from = abs_pos + 11;
        }
        ports
    }
}

impl fmt::Display for PortStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = match self.port_type { PortType::Rj45 => "RJ45", PortType::Fiber => "SFP+" };
        write!(f, "Port {:>2} ({}): {}", self.port, t, if self.link_up { "up" } else { "down" })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PortStats {
    pub port: u8, pub enabled: bool, pub link_up: bool,
    pub tx_good: u64, pub tx_bad: u64, pub rx_good: u64, pub rx_bad: u64,
}

impl PortStats {
    fn parse_table(html: &str) -> Result<Vec<PortStats>, SodolaError> {
        let mut stats = Vec::new();
        let mut search_from = 0;
        while let Some(pos) = html[search_from..].find(">Port ") {
            let abs_pos = search_from + pos;
            let port_str: String = html[abs_pos + 6..].chars().take_while(|c| c.is_ascii_digit()).collect();
            let port: u8 = match port_str.parse() { Ok(p) => p, Err(_) => { search_from = abs_pos + 6; continue; } };
            let tr_start = html[..abs_pos].rfind("<tr").unwrap_or(abs_pos);
            let tr_end = html[tr_start..].find("</tr").unwrap_or(html.len() - tr_start);
            let cells = extract_all_td(&html[tr_start..tr_start + tr_end]);
            if cells.len() >= 7 {
                stats.push(PortStats {
                    port, enabled: cells[1] == "Enable", link_up: cells[2] == "Link Up",
                    tx_good: cells[3].parse().unwrap_or(0), tx_bad: cells[4].parse().unwrap_or(0),
                    rx_good: cells[5].parse().unwrap_or(0), rx_bad: cells[6].parse().unwrap_or(0),
                });
            }
            search_from = abs_pos + 6;
        }
        Ok(stats)
    }
}

impl fmt::Display for PortStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Port {:>2}  {:<8} {:<10} TX: {:>8}/{:<8} RX: {:>8}/{}",
            self.port, if self.enabled { "Enable" } else { "Disable" },
            if self.link_up { "Link Up" } else { "Link Down" },
            self.tx_good, self.tx_bad, self.rx_good, self.rx_bad)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum VlanPortMode { Untagged, Tagged, NotMember }

#[derive(Debug, Clone, Serialize)]
pub struct PortVlanSetting { pub port: u8, pub pvid: u16, pub accepted_frame_type: AcceptedFrameType }

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum AcceptedFrameType { All, TagOnly, UntagOnly }

impl PortVlanSetting {
    fn parse_table(html: &str) -> Result<Vec<PortVlanSetting>, SodolaError> {
        let mut settings = Vec::new();
        let table_start = html.find("<hr>").unwrap_or(0);
        let table_html = &html[table_start..];
        let mut search_from = 0;
        while let Some(pos) = table_html[search_from..].find(">Port ") {
            let abs_pos = search_from + pos;
            let port_str: String = table_html[abs_pos + 6..].chars().take_while(|c| c.is_ascii_digit()).collect();
            let port: u8 = match port_str.parse() { Ok(p) => p, Err(_) => { search_from = abs_pos + 6; continue; } };
            let tr_start = table_html[..abs_pos].rfind("<tr").unwrap_or(abs_pos);
            let tr_end = table_html[tr_start..].find("</tr").unwrap_or(table_html.len() - tr_start);
            let cells = extract_all_td(&table_html[tr_start..tr_start + tr_end]);
            if cells.len() >= 3 {
                settings.push(PortVlanSetting {
                    port, pvid: cells[1].parse().unwrap_or(1),
                    accepted_frame_type: match cells[2].as_str() {
                        "Tag-only" => AcceptedFrameType::TagOnly,
                        "Untag-only" => AcceptedFrameType::UntagOnly,
                        _ => AcceptedFrameType::All,
                    },
                });
            }
            search_from = abs_pos + 6;
        }
        Ok(settings)
    }
}

impl fmt::Display for AcceptedFrameType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self { Self::All => write!(f, "All"), Self::TagOnly => write!(f, "Tag-only"), Self::UntagOnly => write!(f, "Untag-only") }
    }
}

impl fmt::Display for PortVlanSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Port {:>2}  PVID: {:>4}  Accept: {}", self.port, self.pvid, self.accepted_frame_type)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VlanEntry { pub vid: u16, pub name: String, pub member_ports: String, pub tagged_ports: String, pub untagged_ports: String }

impl VlanEntry {
    fn parse_table(html: &str) -> Vec<VlanEntry> {
        let mut vlans = Vec::new();
        let mut search_from = 0;
        while let Some(pos) = html[search_from..].find("pickVlanId=") {
            let abs_pos = search_from + pos;
            let vid_str: String = html[abs_pos + 11..].chars().take_while(|c| c.is_ascii_digit()).collect();
            let vid: u16 = match vid_str.parse() { Ok(v) => v, Err(_) => { search_from = abs_pos + 12; continue; } };
            let tr_start = html[..abs_pos].rfind("<tr").unwrap_or(abs_pos);
            let tr_end = html[tr_start..].find("</tr").unwrap_or(html.len() - tr_start);
            let cells = extract_all_td(&html[tr_start..tr_start + tr_end]);
            if cells.len() >= 5 {
                vlans.push(VlanEntry { vid, name: cells[1].clone(), member_ports: cells[2].clone(), tagged_ports: cells[3].clone(), untagged_ports: cells[4].clone() });
            }
            search_from = abs_pos + 12;
        }
        vlans
    }
}

impl fmt::Display for VlanEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VLAN {:>4}  {:<16}  members: {:<10}  tagged: {:<10}  untagged: {}",
            self.vid, if self.name.is_empty() { "-" } else { &self.name },
            self.member_ports, self.tagged_ports, self.untagged_ports)
    }
}

fn extract_all_td(html: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut search_from = 0;
    while let Some(td_start) = html[search_from..].find("<td") {
        let abs_td = search_from + td_start;
        if let Some(gt) = html[abs_td..].find('>') {
            let content_start = abs_td + gt + 1;
            if let Some(td_end) = html[content_start..].find("</td") {
                results.push(strip_tags(&html[content_start..content_start + td_end]));
                search_from = content_start + td_end + 5;
            } else { break; }
        } else { break; }
    }
    results
}

fn strip_tags(s: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' { in_tag = true; } else if c == '>' { in_tag = false; } else if !in_tag { result.push(c); }
    }
    result.trim().to_string()
}

#[derive(Debug)]
pub enum SodolaError { NotAuthenticated, SessionExpired, Http(String), Parse(String), Io(io::Error) }

impl fmt::Display for SodolaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAuthenticated => write!(f, "not authenticated — run `sodola-switch login` first"),
            Self::SessionExpired => write!(f, "session expired or invalid — run `sodola-switch login` to re-authenticate"),
            Self::Http(msg) => write!(f, "HTTP error: {}", msg),
            Self::Parse(msg) => write!(f, "parse error: {}", msg),
            Self::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for SodolaError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_token() {
        assert_eq!(SodolaClient::auth_token("admin", "admin"), "f6fdffe48c908deb0f4c3bd36c032e72");
    }

    #[test]
    fn test_parse_info() {
        let html = r#"<html><head><title>System Information</title></head><body><center>
<fieldset><legend>System Info</legend><br>
<table>
  <tr><th style="width:150px;">Device Model</th><td style="width:250px;">SL-SWTGW218AS</td></tr>
  <tr><th>MAC Address</th><td>1C:2A:A3:1A:47:42</td></tr>
  <tr><th>IP Address</th><td>192.168.2.1</td></tr>
  <tr><th>Netmask</th><td>255.255.255.0</td></tr>
  <tr><th>Gateway</th><td>192.168.2.254</td></tr>
  <tr><th>Firmware Version</th><td>V1.9</td></tr>
  <tr><th>Firmware Date</th><td>Jan 03 2024</td></tr>
  <tr><th>Hardware Version</th><td>V1.1</td></tr>
</table></fieldset></center></body></html>"#;
        let info = SwitchInfo::parse(html).unwrap();
        assert_eq!(info.device_type, "SL-SWTGW218AS");
        assert_eq!(info.mac_address, "1C:2A:A3:1A:47:42");
        assert_eq!(info.ip_address, "192.168.2.1");
        assert_eq!(info.firmware_version, "V1.9");
        assert_eq!(info.hardware_version, "V1.1");
    }

    #[test]
    fn test_parse_vlans() {
        let html = r#"<html><body>
<table border="1"><tr><th>VLAN</th><th>VLAN Name</th><th>Member Ports</th><th>Tagged Ports</th><th>Untagged Ports</th><th>Delete</th></tr>
<tr><td><a href="/vlan.cgi?page=getVlanEntry&pickVlanId=1">1</a></td><td></td><td nowrap>8-9</td><td nowrap>-</td><td nowrap>8-9</td><td><input type="checkbox" disabled></td></tr>
<tr><td><a href="/vlan.cgi?page=getVlanEntry&pickVlanId=10">10</a></td><td>trusted</td><td nowrap>1,9</td><td nowrap>9</td><td nowrap>1</td><td><input type="checkbox"></td></tr>
<tr><td><a href="/vlan.cgi?page=getVlanEntry&pickVlanId=40">40</a></td><td>lab</td><td nowrap>5-7,9</td><td nowrap>9</td><td nowrap>5-7</td><td><input type="checkbox"></td></tr>
</table></body></html>"#;
        let vlans = VlanEntry::parse_table(html);
        assert_eq!(vlans.len(), 3);
        assert_eq!(vlans[0].vid, 1);
        assert_eq!(vlans[0].name, "");
        assert_eq!(vlans[0].member_ports, "8-9");
        assert_eq!(vlans[1].vid, 10);
        assert_eq!(vlans[1].name, "trusted");
        assert_eq!(vlans[2].vid, 40);
        assert_eq!(vlans[2].member_ports, "5-7,9");
    }

    #[test]
    fn test_parse_port_stats() {
        let html = r#"<html><body><table border="1">
<tr><th>Port</th><th>State</th><th>Link Status</th><th>TxGoodPkt</th><th>TxBadPkt</th><th>RxGoodPkt</th><th>RxBadPkt</th></tr>
<tr><td>Port 1</td><td>Enable</td><td>Link Up</td><td>120</td><td>0</td><td>4869</td><td>0</td></tr>
<tr><td>Port 9</td><td>Enable</td><td>Link Up</td><td>62337</td><td>0</td><td>67013</td><td>0</td></tr>
</table></body></html>"#;
        let stats = PortStats::parse_table(html).unwrap();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].port, 1);
        assert!(stats[0].link_up);
        assert_eq!(stats[0].tx_good, 120);
        assert_eq!(stats[1].rx_good, 67013);
    }

    #[test]
    fn test_parse_pvid() {
        let html = r#"<html><body>
<form method="post" action="/vlan.cgi?page=port_based">stuff</form><hr>
<table border="1"><tr><th>Port</th><th>PVID</th><th>Accepted Frame Type</th></tr>
<tr><td>Port 1</td><td>10</td><td>Untag-only</td></tr>
<tr><td>Port 9</td><td>1</td><td>All</td></tr>
</table></body></html>"#;
        let settings = PortVlanSetting::parse_table(html).unwrap();
        assert_eq!(settings.len(), 2);
        assert_eq!(settings[0].pvid, 10);
        assert_eq!(settings[0].accepted_frame_type, AcceptedFrameType::UntagOnly);
        assert_eq!(settings[1].pvid, 1);
        assert_eq!(settings[1].accepted_frame_type, AcceptedFrameType::All);
    }

    #[test]
    fn test_parse_panel() {
        let html = r#"<html><body>
<img src="/RJ45_up_1.png"><img src="/RJ45_up_0.png"><img src="/Fiber_up_0.png">
</body></html>"#;
        let ports = PortStatus::parse_panel(html);
        assert_eq!(ports.len(), 3);
        assert!(ports[0].link_up);
        assert_eq!(ports[0].port_type, PortType::Rj45);
        assert!(!ports[1].link_up);
        assert!(!ports[2].link_up);
        assert_eq!(ports[2].port_type, PortType::Fiber);
    }
}
