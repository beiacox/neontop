//! Alertas: reglas con umbral (temperaturas, uso, disco lleno, SAI…) que se comprueban con cada
//! muestra, aunque la ventana esté oculta. Al dispararse dan un aviso (`Fired`) que la interfaz
//! muestra dentro de NeonTop y/o como notificación del escritorio, según el canal de la regla.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::history::mem_segments;
use crate::sampler::Snapshot;

/// Dónde avisa una regla.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Both,
    App,
    System,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    pub id: String,
    pub label: String,
    pub unit: String,
    pub enabled: bool,
    pub threshold: f64,
    /// Segundos que debe mantenerse por encima del umbral antes de avisar.
    pub sustain: u32,
    pub channel: Channel,
    /// Minutos entre avisos mientras siga por encima; 0 = una sola vez hasta que se resuelva.
    pub repeat: u32,
    /// Silenciada hasta este instante (ms).
    pub muted_until: u64,
    /// Elementos que no se vigilan (puntos de montaje en «disco lleno», discos en su temperatura).
    pub ignore: Vec<String>,
    /// Avisa cuando el valor BAJA del umbral (batería del SAI).
    pub below: bool,
    /// Regla de sí/no (sin umbral editable): avisa cuando el valor es 1.
    pub flag: bool,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            id: String::new(),
            label: String::new(),
            unit: String::new(),
            enabled: true,
            threshold: 0.0,
            sustain: 0,
            channel: Channel::Both,
            repeat: 0,
            muted_until: 0,
            ignore: Vec::new(),
            below: false,
            flag: false,
        }
    }
}

fn rule(id: &str, label: &str, unit: &str, enabled: bool, threshold: f64, sustain: u32, repeat: u32) -> Rule {
    Rule { id: id.into(), label: label.into(), unit: unit.into(), enabled, threshold, sustain, repeat, ..Default::default() }
}

pub fn defaults() -> Vec<Rule> {
    vec![
        rule("cpuTemp", "Temperatura de CPU", "°C", true, 85.0, 10, 5),
        rule("gpuTemp", "Temperatura de GPU (unión)", "°C", true, 95.0, 10, 5),
        rule("cpuUsage", "Uso de CPU", "%", false, 95.0, 60, 15),
        rule("ram", "Uso de RAM", "%", true, 90.0, 15, 15),
        rule("swap", "Uso de swap", "%", false, 50.0, 30, 15),
        // un disco lleno no se arregla solo en 5 minutos: se avisa una vez
        rule("diskFull", "Sistema de ficheros lleno", "%", true, 90.0, 0, 0),
        rule("diskTemp", "Temperatura de disco", "°C", true, 70.0, 30, 15),
        Rule { flag: true, ..rule("upsOnBatt", "Corte de luz (SAI en batería)", "", true, 1.0, 0, 0) },
        Rule { below: true, ..rule("upsBattery", "Batería del SAI baja", "%", true, 30.0, 0, 0) },
    ]
}

/// Reglas que vigilan varios elementos (se pueden ignorar uno a uno) y cómo se llaman.
pub fn multi(rule: &str) -> Option<&'static str> {
    match rule {
        "diskFull" => Some("sistemas de ficheros"),
        "diskTemp" => Some("discos"),
        _ => None,
    }
}

/// Un valor vigilado: `key` distingue varios elementos de una misma regla (cada disco).
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub key: String,
    pub value: f64,
    pub detail: Option<String>,
}

fn one(value: f64, detail: Option<String>) -> Vec<Item> {
    vec![Item { key: String::new(), value, detail }]
}

/// Valores que vigila una regla en una muestra.
pub fn probe(rule: &str, s: &Snapshot) -> Vec<Item> {
    let opt = |v: Option<f64>| v.map(|v| one(v, None)).unwrap_or_default();
    match rule {
        "cpuTemp" => opt(s.cpu.temp.map(f64::from)),
        "gpuTemp" => s
            .gpus
            .iter()
            .find(|g| g.temp_hotspot.or(g.temp_edge).is_some())
            .map(|g| one(g.temp_hotspot.or(g.temp_edge).unwrap() as f64, Some(g.name.clone())))
            .unwrap_or_default(),
        "cpuUsage" => one(s.cpu.total.usage as f64, None),
        "ram" if s.mem.total > 0 => one(mem_segments(&s.mem).used as f64 / s.mem.total as f64 * 100.0, None),
        "swap" if s.mem.swap_total > 0 => one(s.mem.swap_total.saturating_sub(s.mem.swap_free) as f64 / s.mem.swap_total as f64 * 100.0, None),
        // cada sistema de ficheros por separado: uno lleno no tapa a los demás
        "diskFull" => s
            .disks
            .filesystems
            .iter()
            .map(|f| Item { key: f.mount.clone(), value: f.used as f64 / f.total.max(1) as f64 * 100.0, detail: Some(f.mount.clone()) })
            .collect(),
        "diskTemp" => s
            .disks
            .disks
            .iter()
            .filter(|d| !d.temps.is_empty())
            .map(|d| Item { key: d.name.clone(), value: d.temps.iter().map(|t| t.celsius as f64).fold(f64::MIN, f64::max), detail: Some(d.name.clone()) })
            .collect(),
        "upsOnBatt" => s
            .energy
            .ups
            .as_ref()
            .map(|u| {
                let detail = format!("batería {} % · {} min de autonomía", fmt0(u.battery_pct), fmt0(u.runtime_min));
                one(if u.on_battery { 1.0 } else { 0.0 }, Some(detail))
            })
            .unwrap_or_default(),
        "upsBattery" => s
            .energy
            .ups
            .as_ref()
            .and_then(|u| u.battery_pct.map(|b| one(b, Some(format!("{} min de autonomía", fmt0(u.runtime_min))))))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn fmt0(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.0}")).unwrap_or_else(|| "?".into())
}

/// Margen para dar por resuelta una alerta (evita avisos en bucle si el valor ronda el umbral).
fn hysteresis(unit: &str) -> f64 {
    match unit {
        "%" => 2.0,
        "°C" => 3.0,
        _ => 0.0,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fired {
    pub id: u64,
    pub rule: String,
    /// Elemento concreto (punto de montaje, disco) o "".
    pub key: String,
    pub title: String,
    pub body: String,
    pub ts: u64,
    /// Mostrar dentro de NeonTop / como notificación del escritorio.
    pub toast: bool,
    pub desktop: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Opts {
    /// Notificaciones del escritorio.
    pub desktop: bool,
    /// Avisos emergentes dentro de NeonTop.
    pub toasts: bool,
    /// Todo silenciado hasta este instante (ms).
    pub mute_all: u64,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { desktop: true, toasts: true, mute_all: 0 }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Saved {
    rules: Vec<Rule>,
    opts: Opts,
}

pub struct Alerts {
    path: Option<PathBuf>,
    pub rules: Vec<Rule>,
    pub opts: Opts,
    /// Avisos recientes (registro de Ajustes), más reciente primero.
    pub log: Vec<Fired>,
    /// Avisos visibles dentro de la ventana.
    pub toasts: Vec<Fired>,
    since: HashMap<String, u64>,
    last_fired: HashMap<String, u64>,
    seq: u64,
}

/// Avisos visibles a la vez y tiempo que dura cada uno (ms).
const MAX_TOASTS: usize = 4;
pub const TOAST_MS: u64 = 15_000;

impl Alerts {
    /// `path = None`: solo en memoria (pruebas).
    pub fn load(path: Option<PathBuf>) -> Self {
        let saved: Saved = path.as_deref().map(crate::paths::read_json).unwrap_or_default();
        // se combinan con los valores por defecto: reglas y campos nuevos de versiones posteriores
        let rules = defaults()
            .into_iter()
            .map(|d| match saved.rules.iter().find(|s| s.id == d.id) {
                Some(s) => Rule { label: d.label.clone(), unit: d.unit.clone(), below: d.below, flag: d.flag, ..s.clone() },
                None => d,
            })
            .collect();
        Alerts { path, rules, opts: saved.opts, log: Vec::new(), toasts: Vec::new(), since: HashMap::new(), last_fired: HashMap::new(), seq: 0 }
    }

    pub fn save(&self) {
        if let Some(p) = &self.path {
            let _ = crate::paths::write_json(p, &Saved { rules: self.rules.clone(), opts: self.opts.clone() });
        }
    }

    pub fn update(&mut self, id: &str, f: impl FnOnce(&mut Rule)) {
        if let Some(r) = self.rules.iter_mut().find(|r| r.id == id) {
            f(r);
        }
        self.save();
    }

    pub fn reset(&mut self) {
        self.rules = defaults();
        self.opts = Opts::default();
        self.save();
    }

    pub fn dismiss(&mut self, id: u64) {
        self.toasts.retain(|t| t.id != id);
    }

    /// Quita los avisos que ya llevan `TOAST_MS` en pantalla.
    pub fn expire(&mut self, now: u64) {
        self.toasts.retain(|t| now.saturating_sub(t.ts) < TOAST_MS);
    }

    /// Deja de vigilar un elemento concreto (p. ej. un disco que ya sabes que está lleno).
    pub fn ignore(&mut self, rule: &str, key: &str) {
        if !key.is_empty() {
            self.update(rule, |r| {
                if !r.ignore.iter().any(|k| k == key) {
                    r.ignore.push(key.to_string());
                }
            });
        }
        self.toasts.retain(|t| !(t.rule == rule && t.key == key));
    }

    pub fn unignore(&mut self, rule: &str, key: &str) {
        self.update(rule, |r| r.ignore.retain(|k| k != key));
    }

    /// Silencia una regla `minutes` minutos (0 = reactivar).
    pub fn mute(&mut self, rule: &str, minutes: u64, now: u64) {
        self.update(rule, |r| r.muted_until = if minutes > 0 { now + minutes * 60_000 } else { 0 });
        if minutes > 0 {
            self.toasts.retain(|t| t.rule != rule);
        }
    }

    pub fn mute_all(&mut self, until: u64, now: u64) {
        self.opts.mute_all = until;
        self.save();
        if until > now {
            self.toasts.clear();
        }
    }

    /// Comprueba una muestra. Devuelve los avisos nuevos (para enviar notificaciones).
    pub fn check(&mut self, s: &Snapshot) -> Vec<Fired> {
        let now = s.ts;
        let all_muted = now < self.opts.mute_all;
        let mut fired = Vec::new();
        for r in self.rules.iter().filter(|r| r.enabled) {
            let hyst = hysteresis(&r.unit);
            for it in probe(&r.id, s) {
                if r.ignore.contains(&it.key) {
                    continue;
                }
                let k = format!("{}|{}", r.id, it.key);
                // `below`: el problema es estar por DEBAJO del umbral (batería)
                let over = if r.below { it.value <= r.threshold } else { it.value >= r.threshold };
                if !over {
                    self.since.remove(&k);
                    // resuelta cuando se aleja del umbral con margen: entonces puede volver a avisar
                    let cleared = if r.below { it.value > r.threshold + hyst } else { it.value < r.threshold - hyst };
                    if r.flag || cleared {
                        self.last_fired.remove(&k);
                    }
                    continue;
                }
                let since = *self.since.entry(k.clone()).or_insert(now);
                let held = now.saturating_sub(since) as f64 / 1000.0;
                let due = match self.last_fired.get(&k) {
                    None => true,
                    Some(&last) => r.repeat > 0 && now.saturating_sub(last) >= r.repeat as u64 * 60_000,
                };
                if held < r.sustain as f64 || !due {
                    continue;
                }
                // silenciada: se cuenta como avisada para no soltar todo de golpe al terminar el silencio
                self.last_fired.insert(k, now);
                if all_muted || now < r.muted_until {
                    continue;
                }
                let (title, body) = if r.flag {
                    (format!("⚠ {}", r.label), it.detail.clone().unwrap_or_default())
                } else {
                    let digits = if r.unit == "%" { 0 } else { 1 };
                    let value = format!("{:.*}{}", digits, it.value, r.unit);
                    let detail = it.detail.as_ref().map(|d| format!("{d} · ")).unwrap_or_default();
                    let sustain = if r.sustain > 0 { format!(" durante {} s", r.sustain) } else { String::new() };
                    let cmp = if r.below { "<" } else { ">" };
                    (format!("⚠ {}: {value}", r.label), format!("{detail}umbral {cmp} {}{}{sustain}", r.threshold, r.unit))
                };
                self.seq += 1;
                fired.push(Fired {
                    id: self.seq,
                    rule: r.id.clone(),
                    key: it.key.clone(),
                    title,
                    body,
                    ts: now,
                    toast: self.opts.toasts && r.channel != Channel::System,
                    desktop: self.opts.desktop && r.channel != Channel::App,
                });
            }
        }
        for f in &fired {
            self.log.insert(0, f.clone());
            if f.toast {
                self.toasts.push(f.clone());
            }
        }
        self.log.truncate(50);
        let extra = self.toasts.len().saturating_sub(MAX_TOASTS);
        self.toasts.drain(..extra);
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::disks::FsSnapshot;
    use crate::collectors::ups::UpsSnapshot;

    fn temp(ts: u64, t: f32) -> Snapshot {
        let mut s = Snapshot { ts, ..Default::default() };
        s.cpu.temp = Some(t);
        s
    }

    fn only(a: &mut Alerts, id: &str) {
        for r in &mut a.rules {
            r.enabled = r.id == id;
        }
    }

    #[test]
    fn sustain_repeat_and_hysteresis() {
        let mut a = Alerts::load(None);
        only(&mut a, "cpuTemp"); // 85 °C durante 10 s, repite cada 5 min
        assert!(a.check(&temp(0, 90.0)).is_empty(), "todavía no se ha mantenido");
        assert!(a.check(&temp(5_000, 90.0)).is_empty());
        let f = a.check(&temp(10_000, 90.0));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].title, "⚠ Temperatura de CPU: 90.0°C");
        assert!(f[0].body.contains("durante 10 s"));
        assert!(a.check(&temp(60_000, 90.0)).is_empty(), "no repite antes de 5 min");
        assert_eq!(a.check(&temp(310_000, 90.0)).len(), 1, "repite a los 5 min");
        // baja un poco del umbral (dentro del margen): no se da por resuelta
        a.check(&temp(311_000, 84.0));
        a.check(&temp(312_000, 90.0));
        assert!(a.check(&temp(322_000, 90.0)).is_empty());
        // baja con margen: resuelta, puede volver a avisar
        a.check(&temp(323_000, 70.0));
        a.check(&temp(324_000, 90.0));
        assert_eq!(a.check(&temp(334_000, 90.0)).len(), 1);
        assert_eq!(a.log.len(), 3);
        assert_eq!(a.toasts.len(), 3);
    }

    #[test]
    fn mute_and_channels() {
        let mut a = Alerts::load(None);
        only(&mut a, "cpuTemp");
        a.update("cpuTemp", |r| {
            r.sustain = 0;
            r.channel = Channel::App;
        });
        a.mute("cpuTemp", 10, 0);
        assert!(a.check(&temp(1000, 99.0)).is_empty(), "silenciada");
        a.mute("cpuTemp", 0, 0);
        a.check(&temp(2000, 50.0));
        let f = a.check(&temp(3000, 99.0));
        assert_eq!(f.len(), 1);
        assert!(f[0].toast && !f[0].desktop, "canal solo app");
        a.mute_all(10_000, 3000);
        assert!(a.toasts.is_empty());
        a.expire(1_000_000);
    }

    #[test]
    fn per_item_rules_and_ignore() {
        let mut a = Alerts::load(None);
        only(&mut a, "diskFull");
        let mut s = Snapshot { ts: 0, ..Default::default() };
        s.disks.filesystems = vec![
            FsSnapshot { mount: "/".into(), total: 100, used: 50, ..Default::default() },
            FsSnapshot { mount: "/datos".into(), total: 100, used: 95, ..Default::default() },
        ];
        let f = a.check(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].key, "/datos");
        assert!(a.check(&Snapshot { ts: 99_999_999, ..s.clone() }).is_empty(), "una sola vez (repeat 0)");
        a.ignore("diskFull", "/datos");
        assert!(a.toasts.is_empty());
        assert_eq!(a.rules.iter().find(|r| r.id == "diskFull").unwrap().ignore, vec!["/datos"]);
        a.unignore("diskFull", "/datos");
        assert!(a.rules.iter().find(|r| r.id == "diskFull").unwrap().ignore.is_empty());
        assert_eq!(multi("diskFull"), Some("sistemas de ficheros"));
    }

    #[test]
    fn ups_flag_and_below() {
        let mut a = Alerts::load(None);
        for r in &mut a.rules {
            r.enabled = r.id.starts_with("ups");
        }
        let ups = |on: bool, pct: f64| {
            let mut s = Snapshot::default();
            s.energy.ups = Some(UpsSnapshot { on_battery: on, battery_pct: Some(pct), runtime_min: Some(12.0), ..Default::default() });
            s
        };
        let f = a.check(&ups(true, 80.0));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].title, "⚠ Corte de luz (SAI en batería)");
        assert!(f[0].body.contains("12 min"));
        assert!(a.check(&ups(true, 80.0)).is_empty());
        a.check(&ups(false, 80.0)); // vuelve la luz: se puede volver a avisar
        assert_eq!(a.check(&ups(true, 25.0)).len(), 2, "corte + batería baja");
    }

    #[test]
    fn persistence_merges_with_defaults() {
        let p = std::env::temp_dir().join(format!("neontop-alerts-{}.json", std::process::id()));
        std::fs::write(&p, r#"{"rules":[{"id":"cpuTemp","threshold":70,"label":"viejo"}],"opts":{"desktop":false}}"#).unwrap();
        let a = Alerts::load(Some(p.clone()));
        let r = a.rules.iter().find(|r| r.id == "cpuTemp").unwrap();
        assert_eq!(r.threshold, 70.0);
        assert_eq!(r.label, "Temperatura de CPU", "la etiqueta viene del programa");
        assert_eq!(a.rules.len(), defaults().len());
        assert!(!a.opts.desktop && a.opts.toasts);
        a.save();
        assert_eq!(Alerts::load(Some(p.clone())).rules, a.rules);
        std::fs::remove_file(p).unwrap();
    }
}
