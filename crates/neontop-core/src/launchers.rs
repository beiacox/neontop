//! Medición de FPS automática: añade `neontop-fps` a la configuración de los lanzadores de juegos
//! (Steam, Heroic, Lutris, Prism Launcher) para que cada juego arranque con la capa de NeonTop sin
//! tocar nada a mano.
//!
//! Los lanzadores guardan su configuración al cerrarse y pisarían cualquier cambio hecho mientras
//! están abiertos, así que un hilo vigila y aplica los cambios cuando cada uno está cerrado. Antes
//! de modificar un fichero se guarda una copia `<fichero>.neontop-bak`.

use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::config::ConfigStore;

const WRAPPER: &str = "neontop-fps";
const STEAM_CMD: &str = "%command%";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LauncherStatus {
    pub id: &'static str,
    pub name: &'static str,
    /// Juegos (o ajustes globales) que puede tocar.
    pub total: usize,
    /// Cuántos ya arrancan con `neontop-fps`.
    pub applied: usize,
    /// Abierto: los cambios esperan a que se cierre.
    pub running: bool,
    /// Qué se modifica, para mostrarlo en la interfaz.
    pub scope: &'static str,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FpsAuto {
    pub enabled: bool,
    pub launchers: Vec<LauncherStatus>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// ¿Hay algún proceso de este usuario con este nombre (/proc/<pid>/comm)?
fn running(comms: &[&str]) -> bool {
    let uid = unsafe { libc::getuid() };
    let Ok(dir) = fs::read_dir("/proc") else { return false };
    for e in dir.flatten() {
        let name = e.file_name();
        let Some(pid) = name.to_str().filter(|s| s.bytes().all(|b| b.is_ascii_digit())) else { continue };
        use std::os::unix::fs::MetadataExt;
        if e.metadata().map(|m| m.uid() != uid).unwrap_or(true) {
            continue;
        }
        if let Ok(c) = fs::read_to_string(format!("/proc/{pid}/comm")) {
            if comms.contains(&c.trim_end()) {
                return true;
            }
        }
    }
    false
}

/// Escribe con copia de seguridad y de forma atómica (fichero temporal + rename), conservando permisos.
fn write_file(path: &Path, contents: &str) -> std::io::Result<()> {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".neontop-bak");
    fs::copy(path, &bak)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".neontop-tmp");
    fs::write(&tmp, contents)?;
    if let Ok(m) = fs::metadata(path) {
        let _ = fs::set_permissions(&tmp, m.permissions());
    }
    fs::rename(&tmp, path)
}

// ───────────────────────────── Steam ─────────────────────────────

/// Analizador mínimo del formato KeyValues de Valve (texto). Guarda posiciones en el texto para
/// poder editar solo lo necesario y dejar el resto del fichero byte a byte igual.
struct Kv {
    key: String,
    /// Rango del texto entre comillas del valor (sin unescape: se conserva tal cual).
    value: Option<(usize, usize)>,
    /// Posición de la llave de cierre e hijos, si es un objeto.
    close: usize,
    children: Vec<Kv>,
}

struct KvParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> KvParser<'a> {
    fn skip(&mut self) {
        while self.i < self.s.len() {
            match self.s[self.i] {
                b' ' | b'\t' | b'\r' | b'\n' => self.i += 1,
                b'/' if self.s.get(self.i + 1) == Some(&b'/') => {
                    while self.i < self.s.len() && self.s[self.i] != b'\n' {
                        self.i += 1;
                    }
                }
                _ => break,
            }
        }
    }

    /// Cadena entre comillas (o palabra suelta): devuelve el rango del contenido.
    fn string(&mut self) -> Option<(usize, usize)> {
        self.skip();
        if self.i >= self.s.len() {
            return None;
        }
        if self.s[self.i] == b'"' {
            let start = self.i + 1;
            let mut j = start;
            while j < self.s.len() && self.s[j] != b'"' {
                j += if self.s[j] == b'\\' { 2 } else { 1 };
            }
            if j >= self.s.len() {
                return None;
            }
            self.i = j + 1;
            Some((start, j))
        } else {
            let start = self.i;
            while self.i < self.s.len() && !matches!(self.s[self.i], b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'}' | b'"') {
                self.i += 1;
            }
            (self.i > start).then_some((start, self.i))
        }
    }

    /// Lista de pares hasta `}` (o el final). Devuelve los pares y la posición de la `}`.
    fn list(&mut self) -> Option<(Vec<Kv>, usize)> {
        let mut out = Vec::new();
        loop {
            self.skip();
            if self.i >= self.s.len() {
                return Some((out, self.s.len()));
            }
            if self.s[self.i] == b'}' {
                let close = self.i;
                self.i += 1;
                return Some((out, close));
            }
            let (ks, ke) = self.string()?;
            let key = String::from_utf8_lossy(&self.s[ks..ke]).into_owned();
            self.skip();
            if self.s.get(self.i) == Some(&b'{') {
                self.i += 1;
                let (children, close) = self.list()?;
                out.push(Kv { key, value: None, close, children });
            } else {
                let v = self.string()?;
                out.push(Kv { key, value: Some(v), close: 0, children: Vec::new() });
            }
        }
    }
}

fn kv_parse(text: &str) -> Option<Vec<Kv>> {
    KvParser { s: text.as_bytes(), i: 0 }.list().map(|(v, _)| v)
}

fn kv_get<'a>(list: &'a [Kv], key: &str) -> Option<&'a Kv> {
    list.iter().find(|k| k.key.eq_ignore_ascii_case(key))
}

fn kv_path<'a>(list: &'a [Kv], path: &[&str]) -> Option<&'a Kv> {
    let (first, rest) = path.split_first()?;
    let k = kv_get(list, first)?;
    if rest.is_empty() {
        Some(k)
    } else {
        kv_path(&k.children, rest)
    }
}

/// Sangría (tabuladores) de la línea donde está `pos`.
fn indent_at(text: &str, pos: usize) -> &str {
    let line = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text[line..pos].find(|c| c != '\t' && c != ' ').map(|i| line + i).unwrap_or(pos);
    &text[line..end]
}

fn line_start(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0)
}

/// Opciones de lanzamiento con `neontop-fps` (None si ya lo tienen).
fn steam_add(opts: &str) -> Option<String> {
    if opts.contains(WRAPPER) {
        return None;
    }
    let t = opts.trim();
    Some(if t.is_empty() {
        format!("{WRAPPER} {STEAM_CMD}")
    } else if t.contains(STEAM_CMD) {
        // va justo delante del juego: también detrás de variables o envoltorios que ya hubiera
        opts.replacen(STEAM_CMD, &format!("{WRAPPER} {STEAM_CMD}"), 1)
    } else {
        // sin %command%, Steam añade el texto como argumentos del juego
        format!("{WRAPPER} {STEAM_CMD} {t}")
    })
}

/// Opciones de lanzamiento sin `neontop-fps` (None si no lo tienen).
fn steam_remove(opts: &str) -> Option<String> {
    if !opts.contains(WRAPPER) {
        return None;
    }
    let s = opts.replacen(&format!("{WRAPPER} "), "", 1).replacen(WRAPPER, "", 1);
    Some(if s.trim() == STEAM_CMD { String::new() } else { s })
}

fn steam_roots() -> Vec<PathBuf> {
    let h = home();
    let mut seen = HashSet::new();
    // la versión Flatpak no ve /usr/bin/neontop-fps desde su contenedor: no se toca
    [h.join(".local/share/Steam"), h.join(".steam/steam"), h.join(".steam/root")]
        .into_iter()
        .filter_map(|p| fs::canonicalize(p).ok())
        .filter(|p| p.join("steamapps").is_dir() && seen.insert(p.clone()))
        .collect()
}

/// Juegos instalados (appid), sin herramientas (Proton, Steam Linux Runtime, redistribuibles).
fn steam_games(root: &Path) -> Vec<String> {
    let mut libs = vec![root.to_path_buf()];
    let text = fs::read_to_string(root.join("steamapps/libraryfolders.vdf")).unwrap_or_default();
    if let Some(lf) = kv_parse(&text).as_deref().and_then(|l| kv_get(l, "libraryfolders")) {
        for lib in &lf.children {
            if let Some((s, e)) = kv_get(&lib.children, "path").and_then(|k| k.value) {
                libs.push(PathBuf::from(text[s..e].replace("\\\\", "\\")));
            }
        }
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for lib in libs {
        let Ok(dir) = fs::read_dir(lib.join("steamapps")) else { continue };
        for e in dir.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !n.starts_with("appmanifest_") || !n.ends_with(".acf") {
                continue;
            }
            let Ok(text) = fs::read_to_string(e.path()) else { continue };
            let Some(list) = kv_parse(&text) else { continue };
            let val = |k: &str| kv_path(&list, &["AppState", k]).and_then(|k| k.value).map(|(s, e)| text[s..e].to_string()).unwrap_or_default();
            let (id, name) = (val("appid"), val("name"));
            let tool = ["Proton", "Steam Linux Runtime", "Steamworks Common", "Steamworks SDK", "SteamVR"].iter().any(|t| name.contains(t));
            if !id.is_empty() && !tool && seen.insert(id.clone()) {
                out.push(id);
            }
        }
    }
    out
}

fn steam_configs(root: &Path) -> Vec<PathBuf> {
    let Ok(dir) = fs::read_dir(root.join("userdata")) else { return Vec::new() };
    dir.flatten().map(|e| e.path().join("config/localconfig.vdf")).filter(|p| p.is_file()).collect()
}

/// Aplica (o quita) el envoltorio en un localconfig.vdf. Devuelve el texto nuevo si cambia y el
/// número de juegos que quedan con él.
fn steam_edit(text: &str, games: &[String], add: bool) -> (Option<String>, usize) {
    let Some(list) = kv_parse(text) else { return (None, 0) };
    let Some(apps) = kv_path(&list, &["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]) else { return (None, 0) };
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut applied = 0;
    let mut missing = Vec::new();
    for id in games {
        match kv_get(&apps.children, id) {
            Some(app) => {
                let lo = kv_get(&app.children, "LaunchOptions").and_then(|k| k.value);
                let cur = lo.map(|(s, e)| &text[s..e]).unwrap_or("");
                let new = if add { steam_add(cur) } else { steam_remove(cur) };
                if add && (new.is_some() || cur.contains(WRAPPER)) {
                    applied += 1;
                }
                let Some(new) = new else { continue };
                match lo {
                    Some((s, e)) => edits.push((s, e, new)),
                    None if add => {
                        let at = line_start(text, app.close);
                        let ind = format!("{}\t", indent_at(text, app.close));
                        edits.push((at, at, format!("{ind}\"LaunchOptions\"\t\t\"{new}\"\n")));
                    }
                    None => {}
                }
            }
            None if add => {
                missing.push(id);
                applied += 1;
            }
            None => {}
        }
    }
    if !missing.is_empty() {
        let at = line_start(text, apps.close);
        let ind = format!("{}\t", indent_at(text, apps.close));
        let block: String = missing
            .iter()
            .map(|id| format!("{ind}\"{id}\"\n{ind}{{\n{ind}\t\"LaunchOptions\"\t\t\"{WRAPPER} {STEAM_CMD}\"\n{ind}}}\n"))
            .collect();
        edits.push((at, at, block));
    }
    if edits.is_empty() {
        return (None, applied);
    }
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = text.to_string();
    for (s, e, r) in edits {
        out.replace_range(s..e, &r);
    }
    (Some(out), applied)
}

fn steam(add: Option<bool>) -> Option<LauncherStatus> {
    let roots = steam_roots();
    if roots.is_empty() {
        return None;
    }
    let run = running(&["steam"]);
    let (mut total, mut applied) = (0, 0);
    for root in &roots {
        let games = steam_games(root);
        total += games.len();
        let mut best = 0;
        for cfg in steam_configs(root) {
            let Ok(mut text) = fs::read_to_string(&cfg) else { continue };
            if let (Some(a), false) = (add, run) {
                if let (Some(new), _) = steam_edit(&text, &games, a) {
                    match write_file(&cfg, &new) {
                        Ok(()) => text = new,
                        Err(e) => eprintln!("NeonTop: no se pudo escribir {}: {e}", cfg.display()),
                    }
                }
            }
            // con varias cuentas en el equipo se muestra la más completa
            best = best.max(count_steam(&text, &games));
        }
        applied += best;
    }
    Some(LauncherStatus { id: "steam", name: "Steam", total, applied, running: run, scope: "opciones de lanzamiento de cada juego" })
}

fn count_steam(text: &str, games: &[String]) -> usize {
    let Some(list) = kv_parse(text) else { return 0 };
    let Some(apps) = kv_path(&list, &["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]) else { return 0 };
    games
        .iter()
        .filter(|id| {
            kv_get(&apps.children, id)
                .and_then(|a| kv_get(&a.children, "LaunchOptions"))
                .and_then(|k| k.value)
                .is_some_and(|(s, e)| text[s..e].contains(WRAPPER))
        })
        .count()
}

// ───────────────────────────── Heroic ─────────────────────────────

/// En Heroic cada juego tiene su lista de envoltorios ("wrapperOptions"); los ajustes por defecto
/// (config.json → defaultSettings) se copian a los juegos nuevos.
fn heroic_edit(v: &mut serde_json::Value, add: bool) -> (bool, bool) {
    let Some(list) = v.get_mut("wrapperOptions").and_then(|w| w.as_array_mut()) else { return (false, false) };
    let has = list.iter().any(|w| w.get("exe").and_then(|e| e.as_str()) == Some(WRAPPER));
    match (add, has) {
        (true, false) => {
            list.insert(0, serde_json::json!({ "exe": WRAPPER, "args": "" }));
            (true, true)
        }
        (false, true) => {
            list.retain(|w| w.get("exe").and_then(|e| e.as_str()) != Some(WRAPPER));
            (true, false)
        }
        _ => (false, has),
    }
}

fn heroic(add: Option<bool>) -> Option<LauncherStatus> {
    let dir = home().join(".config/heroic");
    if !dir.join("config.json").is_file() {
        return None;
    }
    let run = running(&["heroic"]);
    let write = add.is_some() && !run;
    let mut files: Vec<(PathBuf, bool)> = vec![(dir.join("config.json"), true)];
    if let Ok(d) = fs::read_dir(dir.join("GamesConfig")) {
        files.extend(d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).map(|p| (p, false)));
    }
    let (mut total, mut applied) = (0, 0);
    for (path, defaults) in files {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let mut changed = false;
        let targets: Vec<&mut serde_json::Value> = match v.as_object_mut() {
            Some(o) if defaults => o.get_mut("defaultSettings").into_iter().collect(),
            // un juego: { "<appName>": { …ajustes… }, "version": …, "explicit": … }
            Some(o) => o.values_mut().filter(|x| x.get("wrapperOptions").is_some()).collect(),
            None => Vec::new(),
        };
        for t in targets {
            let has = if write {
                let (c, has) = heroic_edit(t, add.unwrap_or(true));
                changed |= c;
                has
            } else {
                t.get("wrapperOptions").and_then(|w| w.as_array()).is_some_and(|l| l.iter().any(|w| w.get("exe").and_then(|e| e.as_str()) == Some(WRAPPER)))
            };
            if !defaults {
                total += 1;
                applied += has as usize;
            }
        }
        if changed {
            // Heroic guarda con JSON.stringify(…, null, 2): mismo formato
            if let Ok(s) = serde_json::to_string_pretty(&v) {
                if let Err(e) = write_file(&path, &s) {
                    eprintln!("NeonTop: no se pudo escribir {}: {e}", path.display());
                }
            }
        }
    }
    Some(LauncherStatus { id: "heroic", name: "Heroic", total, applied, running: run, scope: "envoltorio de cada juego y de los juegos nuevos" })
}

// ───────────────────────────── Lutris ─────────────────────────────

/// Lutris: "prefix_command" global en system.yml (Opciones del sistema → Prefijo de comando).
fn lutris_edit(text: &str, add: bool) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let sys = lines.iter().position(|l| l.trim_end() == "system:");
    let block_end = sys.map(|s| lines[s + 1..].iter().position(|l| !l.is_empty() && !l.starts_with(' ')).map(|i| s + 1 + i).unwrap_or(lines.len()));
    let pc = sys.zip(block_end).and_then(|(s, e)| (s + 1..e).find(|&i| lines[i].trim_start().starts_with("prefix_command:")));
    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    match (pc, add) {
        (Some(i), _) => {
            let ind = &lines[i][..lines[i].len() - lines[i].trim_start().len()];
            let old = lines[i].trim_start()["prefix_command:".len()..].trim().trim_matches(|c| c == '\'' || c == '"').to_string();
            let has = old.split_whitespace().any(|w| w == WRAPPER);
            if add == has {
                return None;
            }
            let new = if add {
                if old.is_empty() { WRAPPER.to_string() } else { format!("{WRAPPER} {old}") }
            } else {
                old.split_whitespace().filter(|w| *w != WRAPPER).collect::<Vec<_>>().join(" ")
            };
            if new.is_empty() {
                out.remove(i);
            } else {
                out[i] = format!("{ind}prefix_command: {new}");
            }
        }
        (None, false) => return None,
        (None, true) => match sys {
            Some(s) => {
                let ind = lines.get(s + 1).filter(|l| l.starts_with(' ')).map(|l| &l[..l.len() - l.trim_start().len()]).unwrap_or("  ");
                out.insert(s + 1, format!("{ind}prefix_command: {WRAPPER}"));
            }
            None => {
                out.push("system:".into());
                out.push(format!("  prefix_command: {WRAPPER}"));
            }
        },
    }
    Some(out.join("\n") + "\n")
}

fn lutris(add: Option<bool>) -> Option<LauncherStatus> {
    let dir = home().join(".local/share/lutris");
    if !dir.is_dir() {
        return None;
    }
    let path = dir.join("system.yml");
    let run = running(&["lutris"]);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut has = lutris_edit(&text, true).is_none();
    if let (Some(a), false) = (add, run) {
        if let Some(new) = lutris_edit(&text, a) {
            let res = if path.exists() { write_file(&path, &new) } else { fs::write(&path, &new) };
            match res {
                Ok(()) => has = a,
                Err(e) => eprintln!("NeonTop: no se pudo escribir {}: {e}", path.display()),
            }
        }
    }
    Some(LauncherStatus { id: "lutris", name: "Lutris", total: 1, applied: has as usize, running: run, scope: "prefijo de comando global" })
}

// ───────────────────────────── Prism Launcher ─────────────────────────────

/// Prism: "WrapperCommand" global (Ajustes → Comandos personalizados).
fn prism_edit(text: &str, add: bool) -> Option<String> {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let i = lines.iter().position(|l| l.starts_with("WrapperCommand="));
    let old = i.map(|i| lines[i]["WrapperCommand=".len()..].trim().to_string()).unwrap_or_default();
    let has = old.split_whitespace().any(|w| w == WRAPPER);
    if add == has {
        return None;
    }
    let new = if add {
        if old.is_empty() { WRAPPER.to_string() } else { format!("{WRAPPER} {old}") }
    } else {
        old.split_whitespace().filter(|w| *w != WRAPPER).collect::<Vec<_>>().join(" ")
    };
    match i {
        Some(i) => lines[i] = format!("WrapperCommand={new}"),
        None => {
            // va en la sección [General] (la primera) o al principio si no hay secciones
            let at = lines.iter().position(|l| l.trim() == "[General]").map(|g| g + 1).unwrap_or(0);
            lines.insert(at, format!("WrapperCommand={new}"));
        }
    }
    Some(lines.join("\n") + "\n")
}

fn prism(add: Option<bool>) -> Option<LauncherStatus> {
    let path = home().join(".local/share/PrismLauncher/prismlauncher.cfg");
    let text = fs::read_to_string(&path).ok()?;
    let run = running(&["prismlauncher"]);
    let mut has = prism_edit(&text, true).is_none();
    if let (Some(a), false) = (add, run) {
        if let Some(new) = prism_edit(&text, a) {
            match write_file(&path, &new) {
                Ok(()) => has = a,
                Err(e) => eprintln!("NeonTop: no se pudo escribir {}: {e}", path.display()),
            }
        }
    }
    Some(LauncherStatus { id: "prism", name: "Prism Launcher", total: 1, applied: has as usize, running: run, scope: "envoltorio global de Minecraft" })
}

// ───────────────────────────── Conjunto ─────────────────────────────

/// `add`: Some(true) añade, Some(false) quita, None solo consulta. Cada lanzador abierto se salta.
fn pass(add: Option<bool>) -> Vec<LauncherStatus> {
    [steam(add), heroic(add), lutris(add), prism(add)].into_iter().flatten().collect()
}

pub fn status(cfg: &ConfigStore) -> FpsAuto {
    FpsAuto { enabled: cfg.get().fps_auto, launchers: pass(None) }
}

/// Aplica ya lo que se pueda (los lanzadores abiertos quedan pendientes para el vigilante).
pub fn apply(cfg: &ConfigStore) -> FpsAuto {
    let c = cfg.get();
    let launchers = if c.fps_auto {
        pass(Some(true))
    } else if c.fps_auto_cleanup {
        let l = pass(Some(false));
        if l.iter().all(|x| !x.running) {
            cfg.update(|c| c.fps_auto_cleanup = false);
        }
        l
    } else {
        pass(None)
    };
    FpsAuto { enabled: c.fps_auto, launchers }
}

/// ¿Hay un juego de Steam en marcha? (no se cierra Steam con un juego abierto)
fn steam_game_running() -> bool {
    running(&["reaper", "steam-launch-wr"])
}

/// Cierra Steam, aplica los cambios y lo vuelve a abrir (en segundo plano, a la bandeja).
pub fn restart_steam(cfg: &ConfigStore) -> Result<FpsAuto, String> {
    if steam_game_running() {
        return Err("Hay un juego de Steam abierto: ciérralo primero.".into());
    }
    if running(&["steam"]) {
        std::process::Command::new("steam").arg("-shutdown").spawn().map_err(|e| format!("No se pudo cerrar Steam: {e}"))?;
        let mut waited = 0;
        while running(&["steam"]) {
            std::thread::sleep(Duration::from_millis(500));
            waited += 1;
            if waited > 120 {
                return Err("Steam no se ha cerrado en 60 s.".into());
            }
        }
    }
    let res = apply(cfg);
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new("steam");
    cmd.arg("-silent").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    // en su propia sesión: que no muera con NeonTop
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn().map_err(|e| format!("Cambios aplicados, pero no se pudo volver a abrir Steam: {e}"))?;
    Ok(res)
}

/// Hilo vigilante: cada 20 s aplica lo pendiente en los lanzadores que estén cerrados.
pub fn spawn(cfg: Arc<ConfigStore>) {
    std::thread::Builder::new()
        .name("neontop-launchers".into())
        .spawn(move || loop {
            let c = cfg.get();
            if c.fps_auto || c.fps_auto_cleanup {
                apply(&cfg);
            }
            std::thread::sleep(Duration::from_secs(20));
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"10\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1\"\n\t\t\t\t\t}\n\t\t\t\t\t\"20\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"PROTON_X=1  %command% \"\n\t\t\t\t\t}\n\t\t\t\t\t\"30\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"bash -c 'exec \\\"$@\\\"' -- %command%\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";

    #[test]
    fn steam_roundtrip() {
        let games: Vec<String> = ["10", "20", "30", "40"].iter().map(|s| s.to_string()).collect();
        let (new, n) = steam_edit(CFG, &games, true);
        let new = new.unwrap();
        assert_eq!(n, 4);
        assert!(new.contains("\t\t\t\t\t\t\"LastPlayed\"\t\t\"1\"\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"neontop-fps %command%\"\n\t\t\t\t\t}"));
        assert!(new.contains("\"PROTON_X=1  neontop-fps %command% \""));
        assert!(new.contains("\"bash -c 'exec \\\"$@\\\"' -- neontop-fps %command%\""));
        assert!(new.contains("\t\t\t\t\t\"40\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"neontop-fps %command%\"\n\t\t\t\t\t}\n\t\t\t\t}"));
        assert_eq!(count_steam(&new, &games), 4);
        // segunda pasada: nada que hacer
        assert!(steam_edit(&new, &games, true).0.is_none());
        // quitar deja las opciones como estaban (las vacías, vacías)
        let back = steam_edit(&new, &games, false).0.unwrap();
        assert!(back.contains("\"PROTON_X=1  %command% \""));
        assert!(back.contains("\"bash -c 'exec \\\"$@\\\"' -- %command%\""));
        assert_eq!(count_steam(&back, &games), 0);
    }

    #[test]
    fn steam_opts() {
        assert_eq!(steam_add("-novid").unwrap(), "neontop-fps %command% -novid");
        assert!(steam_add("neontop-fps %command%").is_none());
        assert_eq!(steam_remove("neontop-fps %command%").unwrap(), "");
        assert_eq!(steam_remove("A=1 neontop-fps gamemoderun %command%").unwrap(), "A=1 gamemoderun %command%");
    }

    #[test]
    fn lutris_prism() {
        assert_eq!(lutris_edit("system:\n  mangohud: true\n", true).unwrap(), "system:\n  prefix_command: neontop-fps\n  mangohud: true\n");
        assert_eq!(lutris_edit("system:\n  prefix_command: gamemoderun\n", true).unwrap(), "system:\n  prefix_command: neontop-fps gamemoderun\n");
        assert_eq!(lutris_edit("system:\n  prefix_command: neontop-fps\n  mangohud: true\n", false).unwrap(), "system:\n  mangohud: true\n");
        assert_eq!(lutris_edit("", true).unwrap(), "system:\n  prefix_command: neontop-fps\n");
        assert_eq!(prism_edit("[General]\nA=1\n", true).unwrap(), "[General]\nWrapperCommand=neontop-fps\nA=1\n");
        assert!(prism_edit("WrapperCommand=neontop-fps\n", true).is_none());
        assert_eq!(prism_edit("WrapperCommand=neontop-fps\n", false).unwrap(), "WrapperCommand=\n");
    }
}

