//! NeonTop FPS · OpenGL (GLX y EGL) mediante LD_PRELOAD (lo carga el lanzador `neontop-fps`).
//!
//! Intercepta glXSwapBuffers / eglSwapBuffers para anotar cada fotograma. Muchos juegos (SDL,
//! motores propios) no enlazan esas funciones sino que las piden con dlsym() o
//! glXGetProcAddress(), así que también se interceptan esas búsquedas y se devuelve el
//! envoltorio. Todo lo demás pasa intacto.
//!
//! Convivencia con otras precargas que también interceptan dlsym (overlay de Steam, MangoHud):
//!  - Las búsquedas con RTLD_NEXT se resuelven respecto a QUIEN LLAMA, no respecto a esta
//!    librería. Si se reenviaran tal cual, glibc vería esta librería como llamante y devolvería
//!    al otro interceptor su propia función: el overlay de Steam se llamaba a sí mismo sin fin
//!    (100 % de CPU en steam-launch-wrapper, el juego no llegaba a arrancar). Para saber quién
//!    llama, `dlsym` es un trampolín en ensamblador que pasa la dirección de retorno.
//!  - Si este dlsym vuelve a entrar en el mismo hilo (otro interceptor llamando al dlsym global),
//!    va directo al de glibc: ningún ciclo entre interceptores es posible.
//!  - En los procesos de la cadena de lanzamiento (steam-launch-wrapper, reaper, pressure-vessel,
//!    Proton, wineserver…) no se intercepta nada: solo se deja pasar.
//!  - El dlsym de glibc se localiza leyendo su tabla de símbolos ELF (no con dlopen/dlvsym, que el
//!    overlay también intercepta), y no se llama a open() (el overlay lo intercepta y su open()
//!    llama a dlsym → recursión).
//!
//! `no_std`, sin asignador ni constructor: nada se ejecuta hasta que hace falta.

#![cfg_attr(not(test), no_std)]
#![allow(non_snake_case, non_upper_case_globals, clippy::missing_safety_doc)]

use core::ffi::{c_char, c_int, c_uint, c_ulong, c_void, CStr};
use core::sync::atomic::{AtomicI8, AtomicU8, AtomicUsize, Ordering};

use neontop_fps_shm::{State, API_EGL, API_GLX};

type DlsymFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type GlxSwap = unsafe extern "C" fn(*mut c_void, c_ulong);
type EglSwap = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_uint;
type GetProc = unsafe extern "C" fn(*const c_char) -> *mut c_void;

const RTLD_NEXT: *mut c_void = -1isize as *mut c_void;
const RTLD_DL_LINKMAP: c_int = 2;

#[cfg(target_pointer_width = "64")]
mod elf {
    pub type Addr = u64;
    pub type Tag = i64;
    #[repr(C)]
    pub struct Sym {
        pub st_name: u32,
        pub st_info: u8,
        pub st_other: u8,
        pub st_shndx: u16,
        pub st_value: u64,
        pub st_size: u64,
    }
    #[repr(C)]
    pub struct Dyn {
        pub d_tag: i64,
        pub d_val: u64,
    }
}

#[cfg(target_pointer_width = "32")]
mod elf {
    pub type Addr = u32;
    pub type Tag = i32;
    #[repr(C)]
    pub struct Sym {
        pub st_name: u32,
        pub st_value: u32,
        pub st_size: u32,
        pub st_info: u8,
        pub st_other: u8,
        pub st_shndx: u16,
    }
    #[repr(C)]
    pub struct Dyn {
        pub d_tag: i32,
        pub d_val: u32,
    }
}

use elf::{Addr, Dyn, Sym, Tag};

// mismo tipo que `Dyn::d_tag` en cada arquitectura (i64 en 64 bits, i32 en 32)
const DT_NULL: Tag = 0;
const DT_STRTAB: Tag = 5;
const DT_SYMTAB: Tag = 6;
const DT_GNU_HASH: Tag = 0x6fff_fef5;
const DT_VERSYM: Tag = 0x6fff_fff0;
const STT_GNU_IFUNC: u8 = 10;

#[repr(C)]
struct LinkMap {
    l_addr: Addr,
    l_name: *const c_char,
    l_ld: *const Dyn,
    l_next: *mut LinkMap,
    l_prev: *mut LinkMap,
}

#[repr(C)]
struct RDebug {
    r_version: c_int,
    r_map: *mut LinkMap,
    r_brk: Addr,
    r_state: c_int,
    r_ldbase: Addr,
}

#[repr(C)]
struct DlInfo {
    dli_fname: *const c_char,
    dli_fbase: *mut c_void,
    dli_sname: *const c_char,
    dli_saddr: *mut c_void,
}

unsafe extern "C" {
    static _r_debug: RDebug;
    static program_invocation_short_name: *const c_char;
    fn dlvsym(handle: *mut c_void, symbol: *const c_char, version: *const c_char) -> *mut c_void;
    fn dladdr1(addr: *const c_void, info: *mut DlInfo, extra: *mut *mut c_void, flags: c_int) -> c_int;
}

static REAL_GLX_SWAP: AtomicUsize = AtomicUsize::new(0);
static REAL_EGL_SWAP: AtomicUsize = AtomicUsize::new(0);
static REAL_GLX_GETPROC: AtomicUsize = AtomicUsize::new(0);
static REAL_GLX_GETPROC_ARB: AtomicUsize = AtomicUsize::new(0);
static REAL_EGL_GETPROC: AtomicUsize = AtomicUsize::new(0);

/// Estado del registro de fotogramas, compartido con la capa Vulkan si también está cargada.
static STATE: State = State::new();

#[unsafe(no_mangle)]
pub extern "C" fn neontop_fps_shared_state() -> *const State {
    &STATE
}

fn bytes(p: *const c_char) -> &'static [u8] {
    if p.is_null() {
        return &[];
    }
    // SAFETY: cadenas C del programa o del cargador dinámico, vivas mientras se usan
    unsafe { CStr::from_ptr(p).to_bytes() }
}

unsafe fn versioned(handle: *mut c_void) -> Option<DlsymFn> {
    let alt: &CStr = if cfg!(target_pointer_width = "64") { c"GLIBC_2.2.5" } else { c"GLIBC_2.0" };
    for v in [c"GLIBC_2.34", alt] {
        let f = unsafe { dlvsym(handle, c"dlsym".as_ptr(), v.as_ptr()) };
        if !f.is_null() {
            return Some(unsafe { core::mem::transmute::<*mut c_void, DlsymFn>(f) });
        }
    }
    None
}

/// Busca `name` en la tabla de símbolos dinámicos de un objeto ya cargado (tabla hash GNU), sin
/// llamar a dlsym/dlvsym/dlopen: otros interceptores no pueden meterse en medio. Es lo que hace
/// MangoHud con elfhacks.
unsafe fn elf_lookup(l: *const LinkMap, name: &[u8]) -> *mut c_void {
    unsafe {
        if l.is_null() || (*l).l_ld.is_null() {
            return core::ptr::null_mut();
        }
        let base = (*l).l_addr;
        // en glibc las direcciones de la sección dinámica ya suelen estar reubicadas; si no, se suma la base
        let ptr = |v: Addr| if v < base { v.wrapping_add(base) } else { v } as usize;
        let (mut symtab, mut strtab, mut gnu, mut versym) = (core::ptr::null::<Sym>(), core::ptr::null::<u8>(), core::ptr::null::<u32>(), core::ptr::null::<u16>());
        let mut d = (*l).l_ld;
        while (*d).d_tag != DT_NULL {
            match (*d).d_tag {
                DT_SYMTAB => symtab = ptr((*d).d_val as Addr) as *const Sym,
                DT_STRTAB => strtab = ptr((*d).d_val as Addr) as *const u8,
                DT_GNU_HASH => gnu = ptr((*d).d_val as Addr) as *const u32,
                DT_VERSYM => versym = ptr((*d).d_val as Addr) as *const u16,
                _ => {}
            }
            d = d.add(1);
        }
        if symtab.is_null() || strtab.is_null() || gnu.is_null() {
            return core::ptr::null_mut();
        }
        let (nbuckets, symoffset, bloom) = (*gnu, *gnu.add(1), *gnu.add(2));
        if nbuckets == 0 {
            return core::ptr::null_mut();
        }
        let buckets = gnu.add(4 + bloom as usize * (core::mem::size_of::<Addr>() / 4));
        let chains = buckets.add(nbuckets as usize);
        let h = gnu_hash(name);
        let mut i = *buckets.add((h % nbuckets) as usize);
        if i < symoffset {
            return core::ptr::null_mut();
        }
        // con varias versiones del símbolo se prefiere la de por defecto (las antiguas llevan 0x8000)
        let mut hidden: *mut c_void = core::ptr::null_mut();
        loop {
            let ch = *chains.add((i - symoffset) as usize);
            let sym = &*symtab.add(i as usize);
            if (h | 1) == (ch | 1) && sym.st_shndx != 0 && cstr_eq(strtab.add(sym.st_name as usize), name) {
                let mut addr = base.wrapping_add(sym.st_value) as usize as *mut c_void;
                // IFUNC (memcpy, strlen…): la entrada es el resolutor; dlsym devolvería lo que este elige
                if sym.st_info & 0xf == STT_GNU_IFUNC {
                    let resolver: unsafe extern "C" fn() -> *mut c_void = core::mem::transmute(addr);
                    addr = resolver();
                }
                if versym.is_null() || *versym.add(i as usize) & 0x8000 == 0 {
                    return addr;
                }
                if hidden.is_null() {
                    hidden = addr;
                }
            }
            if ch & 1 != 0 {
                break;
            }
            i += 1;
        }
        hidden
    }
}

/// Hash GNU de un nombre de símbolo.
pub fn gnu_hash(name: &[u8]) -> u32 {
    name.iter().fold(5381u32, |h, &c| h.wrapping_mul(33).wrapping_add(c as u32))
}

unsafe fn cstr_eq(p: *const u8, s: &[u8]) -> bool {
    unsafe {
        for (i, &c) in s.iter().enumerate() {
            if *p.add(i) != c {
                return false;
            }
        }
        *p.add(s.len()) == 0
    }
}

static LIBC_DLSYM: AtomicUsize = AtomicUsize::new(0);
static CHAIN_DLSYM: AtomicUsize = AtomicUsize::new(0);

/// dlsym de glibc, sacado de la tabla de símbolos de libc (no "el siguiente", que podría ser otro interceptor).
unsafe fn libc_dlsym() -> Option<DlsymFn> {
    unsafe {
        let mut f = LIBC_DLSYM.load(Ordering::Relaxed);
        if f == 0 {
            let mut l = _r_debug.r_map;
            while !l.is_null() && f == 0 {
                let name = bytes((*l).l_name);
                let base = name.iter().rposition(|&c| c == b'/').map(|i| &name[i + 1..]).unwrap_or(name);
                if base.starts_with(b"libc.so.6") {
                    f = elf_lookup(l, b"dlsym") as usize;
                }
                l = (*l).l_next;
            }
            if f == 0 {
                f = versioned(RTLD_NEXT).map(|f| f as usize).unwrap_or(0);
            }
            LIBC_DLSYM.store(f, Ordering::Relaxed);
        }
        (f != 0).then(|| core::mem::transmute::<usize, DlsymFn>(f))
    }
}

/// dlsym siguiente en la cadena de precargas (otro interceptor o glibc).
unsafe fn chain_dlsym() -> Option<DlsymFn> {
    unsafe {
        let mut f = CHAIN_DLSYM.load(Ordering::Relaxed);
        if f == 0 {
            f = versioned(RTLD_NEXT).map(|f| f as usize).or_else(|| libc_dlsym().map(|f| f as usize)).unwrap_or(0);
            CHAIN_DLSYM.store(f, Ordering::Relaxed);
        }
        (f != 0).then(|| core::mem::transmute::<usize, DlsymFn>(f))
    }
}

/// Símbolo siguiente respecto a ESTA librería (para encontrar el glXSwapBuffers real).
unsafe fn next_sym(name: &CStr) -> *mut c_void {
    unsafe { libc_dlsym().map(|d| d(RTLD_NEXT, name.as_ptr())).unwrap_or(core::ptr::null_mut()) }
}

/// RTLD_NEXT respecto a `caller`: el primer objeto cargado después del que contiene `caller` que
/// defina `name`. En glibc el handle de un objeto es su link_map.
unsafe fn next_after(caller: *const c_void, name: *const c_char) -> *mut c_void {
    unsafe {
        let mut info = core::mem::zeroed::<DlInfo>();
        let mut lm: *mut c_void = core::ptr::null_mut();
        if dladdr1(caller, &mut info, &mut lm, RTLD_DL_LINKMAP) == 0 || lm.is_null() {
            return libc_dlsym().map(|d| d(RTLD_NEXT, name)).unwrap_or(core::ptr::null_mut());
        }
        // búsqueda directa en la tabla de cada objeto posterior (no dlsym: los objetos del arranque y el
        // vDSO no sirven como handle, y así ningún interceptor se mete en medio)
        let n = bytes(name);
        let mut l = (*(lm as *mut LinkMap)).l_next;
        while !l.is_null() {
            let r = elf_lookup(l, n);
            if !r.is_null() {
                return r;
            }
            l = (*l).l_next;
        }
        core::ptr::null_mut()
    }
}

/// ¿Es un proceso de la cadena de lanzamiento (no el juego)?
pub fn is_launcher(name: &[u8]) -> bool {
    const SKIP: [&[u8]; 13] = [
        b"steam-launch-wrapper",
        b"reaper",
        b"bwrap",
        b"srt-bwrap",
        b"wineserver",
        b"steam",
        b"steamwebhelper",
        b"timeout",
        b"sh",
        b"bash",
        b"zsh",
        b"dash",
        b"env",
    ];
    const PREFIX: [&[u8]; 8] = [b"pv-", b"srt-", b"steam-runtime", b"python", b"proton", b"gamemode", b"mangohud", b"neontop-fps"];
    SKIP.contains(&name) || PREFIX.iter().any(|p| name.starts_with(p))
}

/// Se decide una vez, sin llamar a nada: el nombre sale de program_invocation_short_name.
fn passive() -> bool {
    static STATE: AtomicI8 = AtomicI8::new(-1);
    let s = STATE.load(Ordering::Relaxed);
    if s >= 0 {
        return s == 1;
    }
    // provisional: si algo vuelve a entrar mientras se decide, no intercepta
    STATE.store(1, Ordering::Relaxed);
    // SAFETY: variable de glibc, ya inicializada al cargar el programa
    let p = is_launcher(bytes(unsafe { program_invocation_short_name }));
    STATE.store(p as i8, Ordering::Relaxed);
    p
}

// ── profundidad de llamadas a nuestro dlsym en este hilo (para cortar ciclos) ──

static KEY: AtomicUsize = AtomicUsize::new(0);
static KEY_PHASE: AtomicU8 = AtomicU8::new(0);

fn depth_key() -> Option<libc::pthread_key_t> {
    loop {
        match KEY_PHASE.load(Ordering::Acquire) {
            2 => return Some(KEY.load(Ordering::Relaxed) as libc::pthread_key_t),
            3 => return None,
            0 if KEY_PHASE.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() => {
                let mut k: libc::pthread_key_t = 0;
                // SAFETY: crea una clave de hilo sin destructor
                let ok = unsafe { libc::pthread_key_create(&mut k, None) } == 0;
                KEY.store(k as usize, Ordering::Relaxed);
                KEY_PHASE.store(if ok { 2 } else { 3 }, Ordering::Release);
            }
            // otro hilo está creando la clave
            _ => core::hint::spin_loop(),
        }
    }
}

fn depth_add(delta: isize) -> usize {
    let Some(k) = depth_key() else { return 0 };
    // SAFETY: clave válida; el valor guardado es un entero, no un puntero real
    unsafe {
        let d = (libc::pthread_getspecific(k) as isize + delta).max(0) as usize;
        libc::pthread_setspecific(k, d as *const c_void);
        d
    }
}

// ── funciones exportadas ──

#[unsafe(no_mangle)]
pub unsafe extern "C" fn glXSwapBuffers(dpy: *mut c_void, drawable: c_ulong) {
    unsafe {
        let mut f = REAL_GLX_SWAP.load(Ordering::Relaxed);
        if f == 0 {
            f = next_sym(c"glXSwapBuffers") as usize;
            REAL_GLX_SWAP.store(f, Ordering::Relaxed);
        }
        if !passive() {
            neontop_fps_shm::frame(&STATE, API_GLX);
        }
        if f != 0 {
            core::mem::transmute::<usize, GlxSwap>(f)(dpy, drawable);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn eglSwapBuffers(dpy: *mut c_void, surface: *mut c_void) -> c_uint {
    unsafe {
        let mut f = REAL_EGL_SWAP.load(Ordering::Relaxed);
        if f == 0 {
            f = next_sym(c"eglSwapBuffers") as usize;
            REAL_EGL_SWAP.store(f, Ordering::Relaxed);
        }
        if !passive() {
            neontop_fps_shm::frame(&STATE, API_EGL);
        }
        if f != 0 { core::mem::transmute::<usize, EglSwap>(f)(dpy, surface) } else { 0 }
    }
}

/// Devuelve nuestro envoltorio si `name` es una función que interceptamos.
fn wrap(name: &[u8], real: *mut c_void) -> *mut c_void {
    if real.is_null() || passive() {
        return real;
    }
    // la búsqueda puede devolver nuestra propia función: no es la real
    if real == glXSwapBuffers as *mut c_void || real == eglSwapBuffers as *mut c_void {
        return real;
    }
    match name {
        b"glXSwapBuffers" => {
            REAL_GLX_SWAP.store(real as usize, Ordering::Relaxed);
            glXSwapBuffers as *mut c_void
        }
        b"eglSwapBuffers" => {
            REAL_EGL_SWAP.store(real as usize, Ordering::Relaxed);
            eglSwapBuffers as *mut c_void
        }
        _ => real,
    }
}

unsafe fn getproc(slot: &AtomicUsize, own: &CStr, name: *const c_char) -> *mut c_void {
    unsafe {
        let mut f = slot.load(Ordering::Relaxed);
        if f == 0 {
            f = next_sym(own) as usize;
            slot.store(f, Ordering::Relaxed);
        }
        let r = if f != 0 { core::mem::transmute::<usize, GetProc>(f)(name) } else { core::ptr::null_mut() };
        wrap(bytes(name), r)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn glXGetProcAddress(name: *const c_char) -> *mut c_void {
    unsafe { getproc(&REAL_GLX_GETPROC, c"glXGetProcAddress", name) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn glXGetProcAddressARB(name: *const c_char) -> *mut c_void {
    unsafe { getproc(&REAL_GLX_GETPROC_ARB, c"glXGetProcAddressARB", name) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn eglGetProcAddress(name: *const c_char) -> *mut c_void {
    unsafe { getproc(&REAL_EGL_GETPROC, c"eglGetProcAddress", name) }
}

/// Cuerpo de `dlsym`; `caller` es la dirección de retorno (la pone el trampolín).
unsafe extern "C" fn dlsym_impl(handle: *mut c_void, name: *const c_char, caller: *const c_void) -> *mut c_void {
    unsafe {
        let r = if handle == RTLD_NEXT {
            // "el siguiente después de mí" se resuelve respecto al que llama, no respecto a nosotros
            next_after(caller, name)
        } else {
            let d = if depth_add(1) > 1 { libc_dlsym() } else { chain_dlsym() };
            let r = d.map(|d| d(handle, name)).unwrap_or(core::ptr::null_mut());
            depth_add(-1);
            r
        };
        if r.is_null() || name.is_null() || passive() {
            return r;
        }
        // las búsquedas de *GetProcAddress también se redirigen, para envolver lo que devuelvan
        if r == glXGetProcAddress as *mut c_void || r == glXGetProcAddressARB as *mut c_void || r == eglGetProcAddress as *mut c_void {
            return r;
        }
        match bytes(name) {
            b"glXGetProcAddress" => {
                REAL_GLX_GETPROC.store(r as usize, Ordering::Relaxed);
                glXGetProcAddress as *mut c_void
            }
            b"glXGetProcAddressARB" => {
                REAL_GLX_GETPROC_ARB.store(r as usize, Ordering::Relaxed);
                glXGetProcAddressARB as *mut c_void
            }
            b"eglGetProcAddress" => {
                REAL_EGL_GETPROC.store(r as usize, Ordering::Relaxed);
                eglGetProcAddress as *mut c_void
            }
            n => wrap(n, r),
        }
    }
}

/// `dlsym` exportado: trampolín que añade la dirección de retorno (quién llama) como tercer
/// argumento y salta a `dlsym_impl`.
#[cfg(all(target_arch = "x86_64", not(test)))]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlsym(_handle: *mut c_void, _name: *const c_char) -> *mut c_void {
    // rdi = handle, rsi = name; [rsp] = dirección de retorno → rdx; salto de cola (la pila no cambia)
    core::arch::naked_asm!("mov rdx, [rsp]", "jmp {0}", sym dlsym_impl)
}

#[cfg(all(target_arch = "x86", not(test)))]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlsym(_handle: *mut c_void, _name: *const c_char) -> *mut c_void {
    // cdecl: [esp] = retorno, [esp+4] = handle, [esp+8] = name. Se llama a dlsym_impl(handle,
    // name, retorno) con la pila alineada a 16 y se limpian los tres argumentos.
    core::arch::naked_asm!(
        "mov eax, [esp]",
        "push eax",
        "push dword ptr [esp + 12]",
        "push dword ptr [esp + 12]",
        "call {0}",
        "add esp, 12",
        "ret",
        sym dlsym_impl
    )
}

/// En pruebas no se exporta `dlsym` (se usaría el del propio programa de pruebas).
#[cfg(test)]
pub unsafe fn test_dlsym(handle: *mut c_void, name: *const c_char, caller: *const c_void) -> *mut c_void {
    unsafe { dlsym_impl(handle, name, caller) }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // SAFETY: sin std no hay desenrollado: se termina el proceso
    unsafe { libc::abort() }
}

/// `core` viene precompilado con desenrollado y deja una referencia a este símbolo aunque aquí
/// los pánicos aborten; nunca se llama.
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_gnu() {
        // valores de referencia de la función de hash GNU (dl-hash)
        assert_eq!(gnu_hash(b""), 5381);
        assert_eq!(gnu_hash(b"printf"), 0x156b_2bb8);
        assert_eq!(gnu_hash(b"exit"), 0x7c96_7e3f);
    }

    #[test]
    fn launcher_names() {
        for n in [b"steam-launch-wrapper".as_slice(), b"reaper", b"pv-adverb", b"srt-bwrap", b"proton", b"python3", b"bash", b"wineserver"] {
            assert!(is_launcher(n), "{}", String::from_utf8_lossy(n));
        }
        for n in [b"glxgears".as_slice(), b"noita.exe", b"java", b"steamapps", b"wine64-preloader"] {
            assert!(!is_launcher(n), "{}", String::from_utf8_lossy(n));
        }
    }

    #[test]
    fn finds_libc_dlsym_through_elf_tables() {
        // SAFETY: búsqueda en los objetos cargados del propio proceso de pruebas
        unsafe {
            let ours = libc_dlsym().expect("dlsym de libc") as usize;
            let real = libc::dlsym(libc::RTLD_DEFAULT, c"dlsym".as_ptr()) as usize;
            assert_eq!(ours, real);
            // y un símbolo normal a través del trampolín de pruebas
            let p = test_dlsym(libc::RTLD_DEFAULT, c"getpid".as_ptr(), core::ptr::null());
            assert_eq!(p as usize, libc::dlsym(libc::RTLD_DEFAULT, c"getpid".as_ptr()) as usize);
            assert_eq!(depth_add(0), 0, "la profundidad vuelve a cero");
        }
    }

    #[test]
    fn rtld_next_relative_to_caller() {
        // RTLD_NEXT desde una dirección del ejecutable de pruebas: getpid sale de libc (posterior)
        // SAFETY: búsqueda en objetos cargados
        unsafe {
            let caller = rtld_next_relative_to_caller as *const c_void;
            let p = test_dlsym(RTLD_NEXT, c"getpid".as_ptr(), caller);
            assert_eq!(p as usize, libc::dlsym(libc::RTLD_DEFAULT, c"getpid".as_ptr()) as usize);
        }
    }
}
