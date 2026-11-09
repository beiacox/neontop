//! Formato de cifras para mostrar (unidades binarias para memoria, decimales para red y ancho
//! de banda, duraciones…). Todo en español: coma de miles no, punto decimal como en el resto de
//! la interfaz.

const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

/// Bytes con unidad binaria: `1536` → `"1.5 KiB"`.
pub fn bytes(b: f64) -> String {
    bytes_digits(b, 1)
}

pub fn bytes_digits(b: f64, digits: usize) -> String {
    let (v, i) = scale(b);
    let v = if b < 0.0 { -v } else { v };
    format!("{:.*} {}", if i == 0 { 0 } else { digits }, v, UNITS[i])
}

fn scale(b: f64) -> (f64, usize) {
    let mut v = b.abs();
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    (v, i)
}

/// Velocidad en bytes/s.
pub fn rate(bps: f64) -> String {
    format!("{}/s", bytes(bps))
}

/// Velocidad en bits/s (lo habitual para enlaces de red).
pub fn bits(bps: f64) -> String {
    const U: [&str; 4] = ["b", "Kb", "Mb", "Gb"];
    let mut v = bps.abs() * 8.0;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    format!("{:.*} {}ps", if i == 0 { 0 } else { 1 }, v, U[i])
}

pub fn pct(v: f64, digits: usize) -> String {
    format!("{v:.digits$}%")
}

/// Frecuencia en MHz: `3450` → `"3.45 GHz"`.
pub fn freq(mhz: f64) -> String {
    if mhz >= 1000.0 { format!("{:.2} GHz", mhz / 1000.0) } else { format!("{mhz:.0} MHz") }
}

/// Número compacto: `12345` → `"12.3k"`.
pub fn num(n: f64) -> String {
    if n >= 1e6 {
        format!("{:.1}M", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.1}k", n / 1e3)
    } else {
        format!("{n:.0}")
    }
}

/// Duración `hh:mm:ss`, con días delante si los hay: `"2d 03:04:05"`.
pub fn duration(sec: f64) -> String {
    let sec = sec.max(0.0) as u64;
    let (d, h, m, s) = (sec / 86400, sec % 86400 / 3600, sec % 3600 / 60, sec % 60);
    if d > 0 { format!("{d}d {h:02}:{m:02}:{s:02}") } else { format!("{h:02}:{m:02}:{s:02}") }
}

/// "usado / total" compacto: si comparten unidad aparece una sola vez (`"11.7 / 32.0 GiB"`).
/// Sin decimales a partir de 100 (`"512 MiB"`): tiene que caber dentro de un velocímetro.
pub fn bytes_of(used: f64, total: f64) -> String {
    let short = |b: f64| {
        let (v, i) = scale(b);
        let s = if i == 0 || v >= 100.0 { format!("{}", v.round()) } else { format!("{v:.1}") };
        (s, UNITS[i])
    };
    let (u, uu) = short(used);
    let (t, tu) = short(total);
    if uu == tu { format!("{u} / {t} {tu}") } else { format!("{u} {uu} / {t} {tu}") }
}

/// Ancho de banda en unidades decimales (como los fabricantes y el test de RAM).
pub fn gbs(bps: f64, digits: usize) -> String {
    if bps >= 1e9 { format!("{:.*} GB/s", digits, bps / 1e9) } else { format!("{:.0} MB/s", bps / 1e6) }
}

/// Potencia: `"12.3 W"`, sin decimales a partir de 100 W; `"N/D"` si no se conoce.
pub fn watts(w: Option<f64>) -> String {
    match w {
        None => crate::t!("N/D").into(),
        Some(w) if w >= 100.0 => format!("{w:.0} W"),
        Some(w) => format!("{w:.1} W"),
    }
}

/// Energía en Wh o kWh.
pub fn wh(wh: f64) -> String {
    if wh >= 1000.0 {
        format!("{:.2} kWh", wh / 1000.0)
    } else if wh >= 10.0 {
        format!("{wh:.0} Wh")
    } else {
        format!("{wh:.2} Wh")
    }
}

/// Intervalo de muestreo: `250` → `"250ms"`, `2000` → `"2s"`, `90000` → `"1.5min"`.
pub fn interval(ms: u64) -> String {
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let s = ms as f64 / 1000.0;
    if s < 60.0 {
        if s.fract() == 0.0 { format!("{s:.0}s") } else { format!("{s:.1}s") }
    } else {
        format!("{:.1}min", s / 60.0)
    }
}

/// Ventana de historial: `None` es ilimitada.
pub fn window(sec: Option<u64>) -> String {
    match sec {
        None => "∞".into(),
        Some(s) if s < 3600 => format!("{}m", s / 60),
        Some(s) => format!("{}h", s / 3600),
    }
}

/// Fecha y hora locales `"dd/mm/aaaa hh:mm"` a partir de ms desde epoch.
pub fn datetime(ms: u64) -> String {
    let t = local(ms);
    format!("{:02}/{:02}/{} {:02}:{:02}", t.tm_mday, t.tm_mon + 1, t.tm_year + 1900, t.tm_hour, t.tm_min)
}

/// Hora local `"hh:mm:ss"`.
pub fn clock(ms: u64) -> String {
    let t = local(ms);
    format!("{:02}:{:02}:{:02}", t.tm_hour, t.tm_min, t.tm_sec)
}

/// Día local `"AAAA-MM-DD"` (clave del histórico diario de energía).
pub fn day_key(ms: u64) -> String {
    let t = local(ms);
    format!("{}-{:02}-{:02}", t.tm_year + 1900, t.tm_mon + 1, t.tm_mday)
}

/// Hora local (0–23).
pub fn local_hour(ms: u64) -> usize {
    local(ms).tm_hour as usize
}

fn local(ms: u64) -> libc::tm {
    let secs = (ms / 1000) as libc::time_t;
    // SAFETY: localtime_r solo escribe en `out`
    unsafe {
        let mut out: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut out);
        out
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_units() {
        assert_eq!(bytes(0.0), "0 B");
        assert_eq!(bytes(1023.0), "1023 B");
        assert_eq!(bytes(1536.0), "1.5 KiB");
        assert_eq!(bytes(32.0 * 1024.0 * 1024.0 * 1024.0), "32.0 GiB");
        assert_eq!(bytes(-2048.0), "-2.0 KiB");
        assert_eq!(rate(1024.0 * 1024.0), "1.0 MiB/s");
    }

    #[test]
    fn bit_units() {
        assert_eq!(bits(100.0), "800 bps");
        assert_eq!(bits(125_000.0), "1.0 Mbps");
        assert_eq!(bits(125_000_000.0), "1.0 Gbps");
    }

    #[test]
    fn misc() {
        assert_eq!(freq(3450.0), "3.45 GHz");
        assert_eq!(freq(800.0), "800 MHz");
        assert_eq!(num(12345.0), "12.3k");
        assert_eq!(num(2_500_000.0), "2.5M");
        assert_eq!(num(42.0), "42");
        assert_eq!(duration(3661.0), "01:01:01");
        assert_eq!(duration(2.0 * 86400.0 + 5.0), "2d 00:00:05");
        assert_eq!(pct(42.345, 1), "42.3%");
        assert_eq!(gbs(38.7e9, 1), "38.7 GB/s");
        assert_eq!(gbs(512e6, 1), "512 MB/s");
        assert_eq!(watts(None), "N/D");
        assert_eq!(watts(Some(12.34)), "12.3 W");
        assert_eq!(watts(Some(152.6)), "153 W");
        assert_eq!(wh(1500.0), "1.50 kWh");
        assert_eq!(wh(15.4), "15 Wh");
        assert_eq!(wh(1.234), "1.23 Wh");
    }

    #[test]
    fn used_of_total() {
        let g = 1024.0 * 1024.0 * 1024.0;
        assert_eq!(bytes_of(11.7 * g, 32.0 * g), "11.7 / 32.0 GiB");
        assert_eq!(bytes_of(512.0 * 1024.0 * 1024.0, 16.0 * g), "512 MiB / 16.0 GiB");
    }

    #[test]
    fn intervals_and_windows() {
        assert_eq!(interval(250), "250ms");
        assert_eq!(interval(2000), "2s");
        assert_eq!(interval(1500), "1.5s");
        assert_eq!(interval(90_000), "1.5min");
        assert_eq!(window(None), "∞");
        assert_eq!(window(Some(120)), "2m");
        assert_eq!(window(Some(6 * 3600)), "6h");
    }

    #[test]
    fn local_time_is_consistent() {
        let ms = 1_700_000_000_000;
        let key = day_key(ms);
        assert_eq!(key.len(), 10);
        assert!(local_hour(ms) < 24);
        assert_eq!(clock(ms).len(), 8);
        assert!(datetime(ms).contains(&key[..4]));
    }
}
