use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use super::{read_trim, read_u64};

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IfaceSnapshot {
    pub name: String,
    /// "ethernet" | "wifi" | "loopback" | "virtual"
    pub kind: String,
    pub state: String,
    pub mac: String,
    pub mtu: u32,
    /// Mbps, si el driver lo expone.
    pub speed: Option<u32>,
    pub addrs: Vec<String>,
    pub rx_rate: f64,
    pub tx_rate: f64,
    pub rx_pkt_rate: f64,
    pub tx_pkt_rate: f64,
    pub rx_total: u64,
    pub tx_total: u64,
    pub rx_errs: u64,
    pub tx_errs: u64,
    pub rx_drop: u64,
    pub tx_drop: u64,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NetSnapshot {
    pub ifaces: Vec<IfaceSnapshot>,
    pub tcp_established: u32,
    pub tcp_listen: u32,
    pub udp_sockets: u32,
}

#[derive(Clone, Copy)]
struct Counters {
    rx: u64,
    tx: u64,
    rxp: u64,
    txp: u64,
}

pub struct NetCollector {
    prev: HashMap<String, Counters>,
    prev_at: Instant,
}

fn kind_of(name: &str) -> &'static str {
    let base = Path::new("/sys/class/net").join(name);
    if name == "lo" {
        "loopback"
    } else if base.join("wireless").exists() || base.join("phy80211").exists() {
        "wifi"
    } else if base.join("device").exists() {
        "ethernet"
    } else {
        "virtual"
    }
}

/// Cuenta sockets TCP por estado leyendo /proc/net/tcp{,6}.
fn socket_counts() -> (u32, u32, u32) {
    let (mut est, mut listen) = (0, 0);
    for f in ["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(t) = fs::read_to_string(f) {
            for line in t.lines().skip(1) {
                match line.split_whitespace().nth(3) {
                    Some("01") => est += 1,
                    Some("0A") => listen += 1,
                    _ => {}
                }
            }
        }
    }
    let udp = ["/proc/net/udp", "/proc/net/udp6"]
        .iter()
        .filter_map(|f| fs::read_to_string(f).ok())
        .map(|t| t.lines().count().saturating_sub(1) as u32)
        .sum();
    (est, listen, udp)
}

impl NetCollector {
    pub fn new() -> Self {
        let mut c = NetCollector { prev: HashMap::new(), prev_at: Instant::now() };
        c.sample();
        c
    }

    pub fn sample(&mut self) -> NetSnapshot {
        let now = Instant::now();
        let secs = now.duration_since(self.prev_at).as_secs_f64().max(1e-3);
        self.prev_at = now;

        let mut addrs: HashMap<String, Vec<String>> = HashMap::new();
        if let Ok(list) = if_addrs::get_if_addrs() {
            for a in list {
                addrs.entry(a.name.clone()).or_default().push(a.ip().to_string());
            }
        }

        let text = fs::read_to_string("/proc/net/dev").unwrap_or_default();
        let mut ifaces = Vec::new();
        let mut next = HashMap::new();

        for line in text.lines().skip(2) {
            let Some((name, rest)) = line.split_once(':') else { continue };
            let name = name.trim().to_string();
            let v: Vec<u64> = rest.split_whitespace().map(|x| x.parse().unwrap_or(0)).collect();
            if v.len() < 16 {
                continue;
            }
            let cur = Counters { rx: v[0], rxp: v[1], tx: v[8], txp: v[9] };
            let r = |c: u64, p: u64| c.saturating_sub(p) as f64 / secs;
            let (rx_rate, tx_rate, rx_pkt_rate, tx_pkt_rate) = match self.prev.get(&name) {
                Some(p) => (r(cur.rx, p.rx), r(cur.tx, p.tx), r(cur.rxp, p.rxp), r(cur.txp, p.txp)),
                None => (0.0, 0.0, 0.0, 0.0),
            };
            next.insert(name.clone(), cur);

            let base = Path::new("/sys/class/net").join(&name);
            // speed devuelve -1 o falla con el enlace caído o en interfaces virtuales
            let speed = read_trim(base.join("speed")).and_then(|s| s.parse::<i64>().ok()).filter(|s| *s > 0).map(|s| s as u32);
            ifaces.push(IfaceSnapshot {
                kind: kind_of(&name).into(),
                state: read_trim(base.join("operstate")).unwrap_or_default(),
                mac: read_trim(base.join("address")).unwrap_or_default(),
                mtu: read_u64(base.join("mtu")).unwrap_or(0) as u32,
                speed,
                addrs: addrs.remove(&name).unwrap_or_default(),
                rx_rate,
                tx_rate,
                rx_pkt_rate,
                tx_pkt_rate,
                rx_total: cur.rx,
                tx_total: cur.tx,
                rx_errs: v[2],
                rx_drop: v[3],
                tx_errs: v[10],
                tx_drop: v[11],
                name,
            });
        }
        self.prev = next;

        let (tcp_established, tcp_listen, udp_sockets) = socket_counts();
        NetSnapshot { ifaces, tcp_established, tcp_listen, udp_sockets }
    }
}
