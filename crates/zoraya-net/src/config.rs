//! The `net.*` settings (ADR-0229), read from `/config/settings` with the
//! declarations of `netd`'s description. A bad line is reported once and its
//! setting keeps the default; a condition between keys (`static` needs
//! `net.ip` and `net.gateway`) is reported here too (ADR-0157 rule 5).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use zoraya_abi::net::valid_name;
use zoraya_describe::{list_items, parse_address, parse_prefix};

/// `netd`'s description, whose `setting` lines declare the keys.
pub const DESCRIPTION: &str = include_str!("../../../user/describe/system/netd.cmd");

/// How the interface gets its address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Dhcp,
    Static {
        address: [u8; 4],
        prefix: u8,
        gateway: [u8; 4],
    },
    /// `net.address = static` and what it needs is missing or wrong: no
    /// address (ADR-0229's example).
    Unconfigured,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub mode: Mode,
    /// `net.dns`: empty means the servers DHCP gave.
    pub dns: Vec<[u8; 4]>,
    /// `net.hosts`: `(name, address)`, names valid and unique.
    pub hosts: Vec<(String, [u8; 4])>,
    pub hostname: String,
}

impl Default for Config {
    fn default() -> Config {
        read(b"").0
    }
}

/// The declared keys.
pub fn declared() -> Vec<zoraya_describe::Setting> {
    zoraya_describe::parse(DESCRIPTION.as_bytes())
        .map(|d| d.settings)
        .unwrap_or_default()
}

/// Whether `a` is a usable unicast host address (not unspecified, loopback,
/// multicast, link-local, or the limited broadcast).
pub fn is_unicast(a: [u8; 4]) -> bool {
    a != [0; 4] && a[0] != 127 && a[0] < 224 && !(a[0] == 169 && a[1] == 254)
}

/// A host name for DHCP option 12: one DNS label, lower case.
fn valid_hostname(name: &str) -> bool {
    !name.is_empty() && name.len() <= 63 && valid_name(name) && !name.contains('.')
}

fn in_network(address: [u8; 4], prefix: u8, other: [u8; 4]) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix))
    };
    u32::from_be_bytes(address) & mask == u32::from_be_bytes(other) & mask
}

/// The configuration in `bytes` (a `/config/settings`) and the console lines
/// to print, once each.
pub fn read(bytes: &[u8]) -> (Config, Vec<String>) {
    let d = declared();
    let v = zoraya_settings::read(bytes, &d);
    let mut problems: Vec<String> = v
        .problems
        .iter()
        .map(|p| format!("netd: /config/settings line {}: {}", p.line, p.problem))
        .collect();
    let get = |name: &str| v.get(name).unwrap_or("");
    let mut config = Config {
        mode: Mode::Dhcp,
        dns: Vec::new(),
        hosts: Vec::new(),
        hostname: String::from("zoraya"),
    };
    // `net.dns`: addresses, already checked as a list of addresses.
    for item in list_items(get("net.dns")).unwrap_or_default() {
        if let Some(a) = parse_address(&item)
            && !config.dns.contains(&a)
        {
            config.dns.push(a);
        }
    }
    for item in list_items(get("net.hosts")).unwrap_or_default() {
        let parsed = item
            .split_once('=')
            .and_then(|(n, a)| Some((n, parse_address(a)?)))
            .filter(|(n, _)| valid_name(n));
        match parsed {
            Some((name, address)) if !config.hosts.iter().any(|(n, _)| n == name) => {
                config.hosts.push((name.to_string(), address));
            }
            Some((name, _)) => {
                problems.push(format!("netd: net.hosts: {name} is in the list twice"))
            }
            None => problems.push(format!(
                "netd: net.hosts: {item} is not name=address with a lower-case name"
            )),
        }
    }
    let hostname = get("net.hostname");
    if valid_hostname(hostname) {
        config.hostname = hostname.to_string();
    } else {
        problems.push(format!(
            "netd: net.hostname: {hostname} is not one lower-case label"
        ));
    }
    if get("net.address") == "static" {
        let ip = parse_prefix(get("net.ip"));
        let gateway = parse_address(get("net.gateway"));
        let mut why = Vec::new();
        match ip {
            None => why.push("net.ip is not set"),
            Some((a, p)) if !is_unicast(a) || p > 30 => {
                why.push("net.ip is not a unicast address with a prefix of 30 or less");
            }
            Some(_) => {}
        }
        match (ip, gateway) {
            (_, None) => why.push("net.gateway is not set"),
            (Some((a, p)), Some(g)) if !is_unicast(g) || !in_network(a, p, g) || g == a => {
                why.push("net.gateway is not another address of net.ip's network");
            }
            _ => {}
        }
        match (why.is_empty(), ip, gateway) {
            (true, Some((address, prefix)), Some(gateway)) => {
                config.mode = Mode::Static {
                    address,
                    prefix,
                    gateway,
                };
            }
            _ => {
                config.mode = Mode::Unconfigured;
                problems.push(format!(
                    "netd: net.address = static, but {}",
                    why.join(" and ")
                ));
            }
        }
    }
    (config, problems)
}
