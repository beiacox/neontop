//! NeonTop FPS: formato de la memoria compartida entre el juego y NeonTop, y registro de
//! fotogramas. Lo usan la capa Vulkan y la precarga de OpenGL, que se cargan dentro de procesos
//! ajenos (juegos, Wine, lanzadores): por eso es `no_std`, sin asignador ni inicialización.
//!
//! Cada proceso con la capa activa crea /dev/shm/neontop-fps-<pid> y anota en un anillo el
//! instante (CLOCK_MONOTONIC, ns) de cada fotograma presentado. NeonTop lee el anillo y calcula
//! FPS, tiempos de fotograma, 1 % / 0,1 % bajos, tirones… El coste en el juego es una lectura del
//! reloj y dos escrituras en memoria por fotograma.
//!
//! El formato debe coincidir con `neontop_core::collectors::games`.

#![cfg_attr(not(test), no_std)]

use core::ffi::c_char;
use core::sync::atomic::{AtomicPtr, AtomicU64, AtomicU8, Ordering};

pub const MAGIC: u64 = 0x3153_5046_5450_4e4e; // "NNPTFPS1"
pub const VERSION: u32 = 1;
/// Fotogramas en el anillo (~27 s a 300 FPS).
pub const CAP: usize = 8192;
/// Tamaño de la cabecera (lo que hay antes de `ts`).
pub const HEADER: usize = 112;

pub const API_VULKAN: u32 = 1;
pub const API_GLX: u32 = 2;
pub const API_EGL: u32 = 3;

#[repr(C)]
pub struct Shm {
    pub magic: AtomicU64,
    pub version: u32,
    pub pid: u32,
    pub api: u32,
    pub capacity: u32,
    /// SteamAppId / SteamGameId, 0 si no es de Steam.
    pub steam_app_id: u32,
    /// 32 o 64.
    pub bits: u32,
    /// Ejecutable (sin ruta).
    pub name: [u8; 64],
    pub start_ns: u64,
    /// Fotogramas totales; `ts[count % CAP]` es el siguiente hueco.
    pub count: AtomicU64,
    pub ts: [u64; CAP],
}

/// Estado del proceso: un solo segmento aunque se carguen la capa Vulkan y la de OpenGL a la vez
/// (OpenGL sobre Vulkan con Zink): la primera API que presenta es la que cuenta.
#[repr(C)]
pub struct State {
    shm: AtomicPtr<Shm>,
    /// 0 = sin abrir, 1 = abriéndose, 2 = listo, 3 = falló.
    phase: AtomicU8,
}

impl State {
    pub const fn new() -> Self {
        State { shm: AtomicPtr::new(core::ptr::null_mut()), phase: AtomicU8::new(0) }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

pub fn now_ns() -> u64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime solo escribe en `t`
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}

/// Escribe `n` en decimal al final de `buf` desde `at`; devuelve la nueva posición.
fn put_num(buf: &mut [u8], mut at: usize, mut n: u32) -> usize {
    let mut digits = [0u8; 10];
    let mut k = 0;
    loop {
        digits[k] = b'0' + (n % 10) as u8;
        k += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    while k > 0 && at < buf.len() - 1 {
        k -= 1;
        buf[at] = digits[k];
        at += 1;
    }
    at
}

/// "/dev/shm/neontop-fps-<pid>\0"
pub fn shm_path(pid: u32, out: &mut [u8; 64]) {
    const PREFIX: &[u8] = b"/dev/shm/neontop-fps-";
    out[..PREFIX.len()].copy_from_slice(PREFIX);
    let end = put_num(out, PREFIX.len(), pid);
    out[end] = 0;
}

/// Último componente de una ruta (Wine: "C:\...\Juego.exe" → "Juego.exe").
pub fn basename(s: &[u8]) -> &[u8] {
    let start = s.iter().rposition(|&c| c == b'/' || c == b'\\').map(|i| i + 1).unwrap_or(0);
    &s[start..]
}

/// Lee un fichero pequeño de /proc sin asignar memoria.
fn read_small(path: &[u8], buf: &mut [u8]) -> usize {
    // SAFETY: `path` termina en 0; `buf` es escribible
    unsafe {
        let fd = libc::open(path.as_ptr() as *const c_char, libc::O_RDONLY | libc::O_CLOEXEC);
        if fd < 0 {
            return 0;
        }
        let r = libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());
        libc::close(fd);
        if r > 0 { r as usize } else { 0 }
    }
}

/// Nombre del juego: argv[0] sin ruta; si no se puede, /proc/self/comm.
fn exe_name(out: &mut [u8; 64]) {
    let mut buf = [0u8; 512];
    let n = read_small(b"/proc/self/cmdline\0", &mut buf[..511]);
    let argv0 = &buf[..n.min(511)];
    let argv0 = &argv0[..argv0.iter().position(|&c| c == 0).unwrap_or(argv0.len())];
    let name: &[u8] = if !argv0.is_empty() {
        basename(argv0)
    } else {
        let n = read_small(b"/proc/self/comm\0", &mut buf[..63]);
        let s = &buf[..n];
        s.strip_suffix(b"\n").unwrap_or(s)
    };
    let l = name.len().min(63);
    out[..l].copy_from_slice(&name[..l]);
    out[l] = 0;
}

fn steam_app_id() -> u32 {
    for var in [b"SteamAppId\0".as_slice(), b"SteamGameId\0".as_slice()] {
        // SAFETY: nombre terminado en 0; getenv devuelve NULL o una cadena C
        let v = unsafe { libc::getenv(var.as_ptr() as *const c_char) };
        if !v.is_null() {
            // SAFETY: cadena C válida
            let n = unsafe { libc::strtoul(v, core::ptr::null_mut(), 10) } as u32;
            if n != 0 {
                return n;
            }
        }
    }
    0
}

static mut CLEANUP_PATH: [u8; 64] = [0; 64];

extern "C" fn cleanup() {
    // SAFETY: CLEANUP_PATH se escribe una vez antes de registrar esta función
    unsafe {
        let p = core::ptr::addr_of!(CLEANUP_PATH) as *const c_char;
        if *p != 0 {
            libc::unlink(p);
        }
    }
}

/// Crea el segmento. Devuelve null si no se pudo (se deja de intentar).
fn open_shm(api: u32) -> *mut Shm {
    let mut path = [0u8; 64];
    // SAFETY: getpid no falla
    let pid = unsafe { libc::getpid() } as u32;
    shm_path(pid, &mut path);
    let size = core::mem::size_of::<Shm>();
    // SAFETY: llamadas al sistema con argumentos válidos; se comprueba cada resultado
    unsafe {
        let fd = libc::open(path.as_ptr() as *const c_char, libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC, 0o600);
        if fd < 0 {
            return core::ptr::null_mut();
        }
        if libc::ftruncate(fd, size as libc::off_t) != 0 {
            libc::close(fd);
            return core::ptr::null_mut();
        }
        let m = libc::mmap(core::ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
        libc::close(fd);
        if m == libc::MAP_FAILED {
            return core::ptr::null_mut();
        }
        let s = m as *mut Shm;
        (*s).version = VERSION;
        (*s).pid = pid;
        (*s).api = api;
        (*s).capacity = CAP as u32;
        (*s).bits = (core::mem::size_of::<usize>() * 8) as u32;
        (*s).steam_app_id = steam_app_id();
        exe_name(&mut (*s).name);
        (*s).start_ns = now_ns();
        // el número mágico se escribe el último: NeonTop ignora segmentos a medio preparar
        (*s).magic.store(MAGIC, Ordering::Release);
        core::ptr::addr_of_mut!(CLEANUP_PATH).write(path);
        libc::atexit(cleanup);
        s
    }
}

/// Símbolo con el que las dos librerías comparten el estado: la precarga de OpenGL lo exporta
/// (está en el ámbito global) y la capa Vulkan lo busca con `dlsym(RTLD_DEFAULT, …)`.
pub const SHARED_SYMBOL: &[u8] = b"neontop_fps_shared_state\0";

/// Anota un fotograma presentado con `api`.
pub fn frame(st: &State, api: u32) {
    let mut s = st.shm.load(Ordering::Acquire);
    if s.is_null() {
        match st.phase.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => {
                s = open_shm(api);
                st.shm.store(s, Ordering::Release);
                st.phase.store(if s.is_null() { 3 } else { 2 }, Ordering::Release);
            }
            // otro hilo lo está abriendo (o falló): este fotograma no se cuenta
            Err(_) => return,
        }
        if s.is_null() {
            return;
        }
    }
    // SAFETY: `s` es el mapeo creado arriba, vivo hasta que termina el proceso
    unsafe {
        if (*s).api != api {
            return;
        }
        let c = (*s).count.load(Ordering::Relaxed);
        core::ptr::addr_of_mut!((*s).ts[(c % CAP as u64) as usize]).write_volatile(now_ns());
        (*s).count.store(c + 1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;

    #[test]
    fn layout_matches_the_reader() {
        assert_eq!(offset_of!(Shm, version), 8);
        assert_eq!(offset_of!(Shm, api), 16);
        assert_eq!(offset_of!(Shm, steam_app_id), 24);
        assert_eq!(offset_of!(Shm, name), 32);
        assert_eq!(offset_of!(Shm, start_ns), 96);
        assert_eq!(offset_of!(Shm, count), 104);
        assert_eq!(offset_of!(Shm, ts), HEADER);
        assert_eq!(core::mem::size_of::<Shm>(), HEADER + 8 * CAP);
    }

    #[test]
    fn helpers() {
        let mut p = [0u8; 64];
        shm_path(4210, &mut p);
        assert_eq!(&p[..p.iter().position(|&c| c == 0).unwrap()], b"/dev/shm/neontop-fps-4210");
        shm_path(0, &mut p);
        assert!(p.starts_with(b"/dev/shm/neontop-fps-0\0"));
        assert_eq!(basename(b"C:\\Juegos\\Noita\\noita.exe"), b"noita.exe");
        assert_eq!(basename(b"/usr/bin/glxgears"), b"glxgears");
        assert_eq!(basename(b"vkcube"), b"vkcube");
    }

    #[test]
    fn writes_frames_to_shared_memory() {
        let st = State::new();
        for _ in 0..10 {
            frame(&st, API_VULKAN);
        }
        // otra API en el mismo proceso no cuenta (OpenGL sobre Vulkan)
        frame(&st, API_GLX);
        let s = st.shm.load(Ordering::Acquire);
        assert!(!s.is_null());
        // SAFETY: segmento recién creado por este proceso
        unsafe {
            assert_eq!((*s).magic.load(Ordering::Acquire), MAGIC);
            assert_eq!((*s).count.load(Ordering::Acquire), 10);
            assert_eq!((*s).pid, std::process::id());
            assert_eq!((*s).bits, 64);
            assert!((*s).ts[9] >= (*s).ts[0] && (*s).ts[0] >= (*s).start_ns);
            let name = &(*s).name;
            assert!(name[0] != 0, "nombre del ejecutable");
        }
        let mut p = [0u8; 64];
        shm_path(std::process::id(), &mut p);
        let path = std::str::from_utf8(&p[..p.iter().position(|&c| c == 0).unwrap()]).unwrap();
        assert_eq!(std::fs::metadata(path).unwrap().len() as usize, core::mem::size_of::<Shm>());
        std::fs::remove_file(path).unwrap();
    }
}
