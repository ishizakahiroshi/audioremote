//! Small helpers for network discovery / share URL construction. Kept in a
//! standalone module so both `main` (startup banner) and `server` (JSON API)
//! can use the same logic.

use serde::Serialize;

use crate::config::Config;

/// One LAN URL the user might share, paired with the interface name so they
/// can pick the right one when multiple NICs exist (physical vs virtual
/// switch on the same host).
#[derive(Debug, Clone, Serialize)]
pub struct ShareEntry {
    pub url: String,
    pub interface: String,
    pub virtual_iface: bool,
}

/// Return every non-loopback IPv4 address on this machine, tagged with the
/// interface name (empty string if unavailable). IPv6 is omitted in v0.1.
pub fn list_lan_ipv4() -> Vec<(String, std::net::Ipv4Addr)> {
    match local_ip_address::list_afinet_netifas() {
        Ok(v) => v
            .into_iter()
            .filter_map(|(name, ip)| match ip {
                std::net::IpAddr::V4(a)
                    if !a.is_loopback() && !a.is_unspecified() && !a.is_link_local() =>
                {
                    Some((name, a))
                }
                _ => None,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Heuristic: does this interface name look like a virtual switch (Hyper-V,
/// WSL, Docker, VirtualBox, Bluetooth PAN, etc.) rather than a physical NIC?
/// Used only for sort order — never filters anything out.
pub fn is_virtual_iface(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("vethernet")
        || n.contains("virtual")
        || n.contains("wsl")
        || n.contains("docker")
        || n.contains("hyper-v")
        || n.contains("vmware")
        || n.contains("vbox")
        || n.contains("virtualbox")
        || n.contains("bluetooth")
        || n.contains("loopback")
        || n.contains("tap-")
        || n.contains("tun")
}

/// Share entries with the token pre-embedded. Physical NICs first, virtual
/// switches last. Empty when LAN is not exposed.
pub fn build_share_entries(cfg: &Config) -> Vec<ShareEntry> {
    if !cfg.lan_exposed() {
        return Vec::new();
    }
    let port = cfg.server.port;
    let token = &cfg.auth.token;
    let mut entries: Vec<ShareEntry> = list_lan_ipv4()
        .into_iter()
        .map(|(iface, ip)| ShareEntry {
            url: format!("http://{ip}:{port}/#t={token}"),
            virtual_iface: is_virtual_iface(&iface),
            interface: iface,
        })
        .collect();
    // Stable sort by `virtual_iface` so physical NICs come first without
    // disturbing the OS enumeration order among peers.
    entries.sort_by_key(|e| e.virtual_iface);
    entries
}

/// The URL a user on the host itself should open (loopback, auth bypassed).
pub fn build_host_url(cfg: &Config) -> String {
    format!("http://127.0.0.1:{}/", cfg.server.port)
}
