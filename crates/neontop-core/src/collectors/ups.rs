//! SAI (UPS): estado y consumo de lo que tiene enchufado, leído de `apcupsd` por su servidor
//! de red local (NIS, 127.0.0.1:3551; es lo mismo que consulta `apcaccess`).
//!
//! La potencia sale de la carga (LOADPCT, % de la nominal) × la potencia nominal (NOMPOWER), o de
//! la potencia real si el modelo la da. Mide **todo lo enchufado al SAI** (PC, monitores…), así
//! que es el consumo total del puesto. Muchos Back-UPS dan la carga en % enteros: la potencia va
//! a saltos de NOMPOWER/100.
//!
//! El servidor de apcupsd responde en un hilo aparte: si el servicio se para o tarda, el
//! muestreo no se bloquea.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpsSnapshot {
    /// "apcupsd"
    pub source: String,
    pub model: String,
    /// Estado tal cual lo da el SAI ("ONLINE", "ONBATT", "ONLINE CHARGING", "ONBATT LOWBATT"…).
    pub status: String,
    pub on_battery: bool,
    pub low_battery: bool,
    /// Carga (% de la potencia nominal).
    pub load_pct: Option<f64>,
    /// Potencia nominal (W).
    pub nominal_w: Option<f64>,
    /// Potencia que entrega ahora (W).
    pub power_w: Option<f64>,
    pub line_v: Option<f64>,
    pub output_v: Option<f64>,
    pub line_hz: Option<f64>,
    /// Carga de la batería (%).
    pub battery_pct: Option<f64>,
    /// Autonomía estimada (minutos).
    pub runtime_min: Option<f64>,
    pub battery_v: Option<f64>,
    pub battery_nominal_v: Option<f64>,
    /// Veces que pasó a batería desde que arrancó apcupsd, y el motivo de la última.
    pub transfers: Option<u32>,
    pub last_transfer: Option<String>,
    /// Segundos en batería ahora y acumulados.
    pub on_battery_s: Option<f64>,
    pub on_battery_total_s: Option<f64>,
    pub battery_date: Option<String>,
    pub self_test: Option<String>,
    /// Temperatura interna (solo algunos modelos).
    pub temp_c: Option<f64>,
}

const POLL: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_millis(800);

/// Consulta `status` al servidor de apcupsd: cada mensaje va precedido de su longitud (u16, BE).
fn query(addr: &SocketAddr) -> std::io::Result<HashMap<String, String>> {
    let mut s = TcpStream::connect_timeout(addr, TIMEOUT)?;
    s.set_read_timeout(Some(TIMEOUT))?;
    s.set_write_timeout(Some(TIMEOUT))?;
    s.write_all(&[0, 6])?;
    s.write_all(b"status")?;
    let mut out = HashMap::new();
    loop {
        let mut len = [0u8; 2];
        s.read_exact(&mut len)?;
        let n = u16::from_be_bytes(len) as usize;
        if n == 0 {
            break;
        }
        let mut buf = vec![0u8; n];
        s.read_exact(&mut buf)?;
        let line = String::from_utf8_lossy(&buf);
        if let Some((k, v)) = line.split_once(':') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Ok(out)
}

/// Primer número de un valor ("121.0 Volts" → 121.0).
fn num(m: &HashMap<String, String>, k: &str) -> Option<f64> {
    m.get(k)?.split_whitespace().next()?.parse().ok()
}

fn text(m: &HashMap<String, String>, k: &str) -> Option<String> {
    m.get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && v != "N/A")
}

fn parse(m: &HashMap<String, String>) -> UpsSnapshot {
    let status = text(m, "STATUS").unwrap_or_default();
    let load = num(m, "LOADPCT");
    let nominal = num(m, "NOMPOWER");
    // algunos modelos dan la potencia real; si no, carga × nominal
    let power = num(m, "REALPOWER").or_else(|| Some(load? / 100.0 * nominal?));
    UpsSnapshot {
        source: "apcupsd".into(),
        model: text(m, "MODEL").unwrap_or_else(|| "SAI".into()),
        on_battery: status.contains("ONBATT"),
        low_battery: status.contains("LOWBATT"),
        status,
        load_pct: load,
        nominal_w: nominal,
        power_w: power,
        line_v: num(m, "LINEV"),
        output_v: num(m, "OUTPUTV"),
        line_hz: num(m, "LINEFREQ"),
        battery_pct: num(m, "BCHARGE"),
        runtime_min: num(m, "TIMELEFT"),
        battery_v: num(m, "BATTV"),
        battery_nominal_v: num(m, "NOMBATTV"),
        transfers: num(m, "NUMXFERS").map(|v| v as u32),
        last_transfer: text(m, "LASTXFER"),
        on_battery_s: num(m, "TONBATT"),
        on_battery_total_s: num(m, "CUMONBATT"),
        battery_date: text(m, "BATTDATE"),
        self_test: text(m, "SELFTEST"),
        temp_c: num(m, "ITEMP"),
    }
}

pub struct UpsCollector {
    latest: Arc<Mutex<Option<(UpsSnapshot, Instant)>>>,
}

impl UpsCollector {
    pub fn new() -> Self {
        let latest: Arc<Mutex<Option<(UpsSnapshot, Instant)>>> = Arc::new(Mutex::new(None));
        // NEONTOP_APCUPSD=host:puerto para un apcupsd en otro equipo
        let addr: SocketAddr = std::env::var("NEONTOP_APCUPSD").ok().and_then(|a| a.parse().ok()).unwrap_or_else(|| "127.0.0.1:3551".parse().unwrap());
        let shared = latest.clone();
        thread::Builder::new()
            .name("ups".into())
            .spawn(move || loop {
                match query(&addr) {
                    Ok(m) if m.contains_key("STATUS") => {
                        *shared.lock().unwrap() = Some((parse(&m), Instant::now()));
                        thread::sleep(POLL);
                    }
                    // sin apcupsd (o sin SAI): se reintenta cada poco, sin coste apreciable
                    _ => {
                        *shared.lock().unwrap() = None;
                        thread::sleep(Duration::from_secs(10));
                    }
                }
            })
            .expect("hilo del SAI");
        UpsCollector { latest }
    }

    /// Último estado conocido (si es reciente).
    pub fn sample(&self) -> Option<UpsSnapshot> {
        let l = self.latest.lock().unwrap();
        l.as_ref().filter(|(_, t)| t.elapsed() < Duration::from_secs(15)).map(|(s, _)| s.clone())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_apcupsd() {
        let addr = "127.0.0.1:3551".parse().unwrap();
        match super::query(&addr) {
            Ok(m) => {
                let u = super::parse(&m);
                eprintln!("{} [{}] carga {:?}% × {:?} W = {:?} W · batería {:?}% · {:?} min · red {:?} V", u.model, u.status, u.load_pct, u.nominal_w, u.power_w, u.battery_pct, u.runtime_min, u.line_v);
            }
            Err(e) => eprintln!("sin apcupsd: {e}"),
        }
    }
}
