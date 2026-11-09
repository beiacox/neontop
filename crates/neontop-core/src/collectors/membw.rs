//! Tráfico de la RAM en tiempo real. Lo mide el programa `neontop-membw` (packaging/membw),
//! que es lo único con permiso para leer los contadores del controlador de memoria
//! (CAP_PERFMON); aquí se lanza, se lee su salida en un hilo y se calcula la velocidad.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::helper::{MEMBW_CONF, MEMBW_PATH};

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemBwSnapshot {
    /// "ok" | "nomodule" | "noperm" | "nopmu" | "unsupported" | "missing" | "error" | "starting"
    pub status: String,
    pub message: Option<String>,
    /// "amd_df" | "intel_imc"
    pub source: Option<String>,
    /// Bytes/s leídos y escritos (solo si el hardware los separa) y total.
    pub read: Option<f64>,
    pub write: Option<f64>,
    pub total: Option<f64>,
    /// El usuario lo activó de forma permanente (módulo al arrancar + permiso).
    pub enabled: bool,
}

#[derive(Default)]
struct Shared {
    status: String,
    message: Option<String>,
    source: Option<String>,
    split: bool,
    /// Última lectura acumulada: (ns, leídos, escritos, total).
    last: Option<(u64, f64, f64, f64)>,
}

pub struct MemBw {
    shared: Arc<Mutex<Shared>>,
    child: Option<Child>,
    prev: Option<(u64, f64, f64, f64)>,
    /// Última velocidad calculada (leídos, escritos, total).
    rates: (Option<f64>, Option<f64>, Option<f64>),
}

/// Ruta del programa: la instalada o, en desarrollo, la compilada en packaging/membw.
fn helper_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NEONTOP_MEMBW").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    let installed = PathBuf::from(MEMBW_PATH);
    if installed.exists() {
        return Some(installed);
    }
    // en desarrollo: el binario compilado en el espacio de trabajo
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target");
    ["release", "debug"].into_iter().map(|p| target.join(p).join("neontop-membw")).find(|p| p.exists())
}

impl Default for MemBw {
    fn default() -> Self {
        Self::new()
    }
}

impl MemBw {
    pub fn new() -> Self {
        let mut m = MemBw { shared: Default::default(), child: None, prev: None, rates: (None, None, None) };
        m.restart();
        m
    }

    /// (Re)lanza el programa, p. ej. después de conceder el permiso.
    pub fn restart(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.prev = None;
        self.rates = (None, None, None);
        *self.shared.lock().unwrap() = Shared { status: "starting".into(), ..Default::default() };
        let Some(path) = helper_path() else {
            self.shared.lock().unwrap().status = "missing".into();
            return;
        };
        let child = Command::new(path).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let mut s = self.shared.lock().unwrap();
                s.status = "error".into();
                s.message = Some(e.to_string());
                return;
            }
        };
        let stdout = child.stdout.take().expect("stdout con tubería");
        let shared = self.shared.clone();
        thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            // primera línea: estado
            if let Some(Ok(first)) = lines.next() {
                let mut parts = first.splitn(3, ' ');
                let mut s = shared.lock().unwrap();
                match (parts.next(), parts.next(), parts.next()) {
                    (Some("ok"), Some(src), split) => {
                        s.status = "ok".into();
                        s.source = Some(src.into());
                        s.split = split == Some("1");
                    }
                    (Some("err"), Some(code), msg) => {
                        s.status = code.into();
                        s.message = msg.map(String::from);
                    }
                    _ => s.status = "error".into(),
                }
            }
            for line in lines.map_while(Result::ok) {
                let v: Vec<f64> = line.split(' ').filter_map(|x| x.parse().ok()).collect();
                if v.len() == 4 {
                    shared.lock().unwrap().last = Some((v[0] as u64, v[1], v[2], v[3]));
                }
            }
            // el programa terminó
            let mut s = shared.lock().unwrap();
            if s.status == "ok" {
                s.status = "error".into();
                s.message = Some(crate::t!("el lector de contadores se detuvo").into());
            }
        });
        self.child = Some(child);
    }

    /// Velocidad desde la muestra anterior (bytes/s).
    pub fn sample(&mut self) -> MemBwSnapshot {
        let s = self.shared.lock().unwrap();
        let mut snap = MemBwSnapshot {
            status: s.status.clone(),
            message: s.message.clone(),
            source: s.source.clone(),
            enabled: std::path::Path::new(MEMBW_CONF).exists(),
            ..Default::default()
        };
        // el programa lee cada 250 ms: con un muestreo más rápido puede no haber lectura nueva,
        // y entonces se repite la última velocidad
        if let (Some(cur), Some(prev)) = (s.last, self.prev) {
            let dt = cur.0.saturating_sub(prev.0) as f64 / 1e9;
            if dt > 0.0 {
                let rate = |a: f64, b: f64| ((a - b) / dt).max(0.0);
                self.rates = (s.split.then(|| rate(cur.1, prev.1)), s.split.then(|| rate(cur.2, prev.2)), Some(rate(cur.3, prev.3)));
            }
        }
        if s.last.is_some() && s.last != self.prev {
            self.prev = s.last;
        }
        (snap.read, snap.write, snap.total) = self.rates;
        snap
    }
}

impl Drop for MemBw {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
        }
    }
}
