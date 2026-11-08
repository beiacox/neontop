//! neontop-fps: lanza un programa con la medición de FPS de NeonTop activada.
//!
//!   En Steam, opciones de lanzamiento:  neontop-fps %command%
//!   Combinable con otros lanzadores:    neontop-fps gamemoderun mangohud %command%
//!
//! - Siempre: NEONTOP_FPS=1 activa la capa Vulkan implícita (juegos Vulkan y Proton/Wine).
//! - OpenGL nativo: además se precarga libneontop_gl.so delante de otras precargas (MangoHud,
//!   overlay de Steam), que la librería después les pasa.
//! - Proton / Wine / Steam Linux Runtime: NO se precarga nada. %command% es toda la cadena de
//!   Steam (steam-launch-wrapper, reaper, pressure-vessel, Proton…) y la precarga la heredarían
//!   procesos que no son el juego; el juego dibuja con Vulkan (DXVK, VKD3D) y lo mide la capa.

use std::ffi::OsString;
use std::os::unix::process::CommandExt;

/// Ruta de la precarga: `$LIB` lo expande el cargador dinámico a lib o lib32 según el programa
/// sea de 64 o 32 bits (no es una variable del shell).
pub const PRELOAD: &str = "/usr/$LIB/neontop/libneontop_gl.so";

/// ¿Se lanza un juego de Proton/Wine (o el runtime de Steam)?
pub fn is_proton(args: &[OsString], env: impl Fn(&str) -> Option<OsString>) -> bool {
    let set = |k: &str| env(k).is_some_and(|v| !v.is_empty());
    if set("STEAM_COMPAT_DATA_PATH") || set("STEAM_COMPAT_CLIENT_INSTALL_PATH") || set("WINEPREFIX") {
        return true;
    }
    args.iter().any(|a| {
        let a = a.to_string_lossy();
        a.contains("proton") || a.contains("Proton") || a.contains("SteamLinuxRuntime") || a.contains("_v2-entry-point") || a.contains("wine") || a.ends_with(".exe") || a.ends_with(".EXE")
    })
}

/// LD_PRELOAD con nuestra librería delante de lo que hubiera.
pub fn preload(current: Option<OsString>) -> OsString {
    match current.filter(|c| !c.is_empty()) {
        Some(c) => {
            let mut s = OsString::from(PRELOAD);
            s.push(":");
            s.push(c);
            s
        }
        None => PRELOAD.into(),
    }
}

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        eprintln!("Uso: neontop-fps <programa> [argumentos…]\n\nEn Steam, opciones de lanzamiento: neontop-fps %command%");
        std::process::exit(if args.is_empty() { 1 } else { 0 });
    }
    let mut cmd = std::process::Command::new(&args[0]);
    cmd.args(&args[1..]).env("NEONTOP_FPS", "1");
    if !is_proton(&args, |k| std::env::var_os(k)) {
        cmd.env("LD_PRELOAD", preload(std::env::var_os("LD_PRELOAD")));
    }
    // exec: el juego sustituye a este proceso (mismo pid, sin un proceso intermedio)
    let err = cmd.exec();
    eprintln!("neontop-fps: no se pudo ejecutar {}: {err}", args[0].to_string_lossy());
    std::process::exit(127);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn detects_proton_and_wine() {
        let none = |_: &str| None;
        assert!(!is_proton(&a(&["glxgears"]), none));
        assert!(!is_proton(&a(&["/home/u/.local/share/Steam/ubuntu12_32/steam-launch-wrapper", "--", "/juegos/factorio/bin/x64/factorio"]), none));
        assert!(is_proton(&a(&["steam-launch-wrapper", "--", "reaper", "SteamLaunch", "AppId=881100", "--", "/x/SteamLinuxRuntime_sniper/_v2-entry-point", "--verb=waitforexitandrun", "--", "/x/proton-cachyos-slr/proton", "waitforexitandrun", "/j/noita.exe"]), none));
        assert!(is_proton(&a(&["wine", "juego.exe"]), none));
        assert!(is_proton(&a(&["./JUEGO.EXE"]), none));
        assert!(is_proton(&a(&["juego"]), |k| (k == "WINEPREFIX").then(|| OsString::from("/home/u/.wine"))));
        assert!(!is_proton(&a(&["juego"]), |k| (k == "WINEPREFIX").then(|| OsString::from(""))));
    }

    #[test]
    fn preload_goes_first() {
        assert_eq!(preload(None), PRELOAD);
        assert_eq!(preload(Some("".into())), PRELOAD);
        assert_eq!(preload(Some("/usr/lib/mangohud/libMangoHud.so".into())), format!("{PRELOAD}:/usr/lib/mangohud/libMangoHud.so").as_str());
    }
}
