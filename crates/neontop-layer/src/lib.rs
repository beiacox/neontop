//! NeonTop FPS · capa Vulkan implícita. Solo se activa con NEONTOP_FPS=1 (ver el manifiesto) y
//! solo intercepta vkQueuePresentKHR para anotar cuándo se presenta cada fotograma; todo lo demás
//! pasa directamente a la siguiente capa o al driver. Cubre los juegos Vulkan nativos y los de
//! Proton/Wine (DXVK y VKD3D traducen DirectX a Vulkan).
//!
//! `no_std`: se carga dentro de cualquier programa Vulkan; no usa asignador ni inicialización.

#![cfg_attr(not(test), no_std)]
#![allow(non_snake_case, clippy::missing_safety_doc)]

use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use neontop_fps_shm::{State, API_VULKAN, SHARED_SYMBOL};

type VkResult = i32;
const VK_SUCCESS: VkResult = 0;
const VK_ERROR_INITIALIZATION_FAILED: VkResult = -3;
const VK_ERROR_DEVICE_LOST: VkResult = -4;
const LOADER_INSTANCE_CREATE_INFO: i32 = 47;
const LOADER_DEVICE_CREATE_INFO: i32 = 48;
const LAYER_LINK_INFO: i32 = 0;

type Handle = *mut c_void;
type VoidFn = Option<unsafe extern "C" fn()>;
type Gipa = unsafe extern "C" fn(Handle, *const c_char) -> VoidFn;
type Gdpa = unsafe extern "C" fn(Handle, *const c_char) -> VoidFn;
type CreateInstance = unsafe extern "C" fn(*const BaseIn, *const c_void, *mut Handle) -> VkResult;
type DestroyInstance = unsafe extern "C" fn(Handle, *const c_void);
type CreateDevice = unsafe extern "C" fn(Handle, *const BaseIn, *const c_void, *mut Handle) -> VkResult;
type DestroyDevice = unsafe extern "C" fn(Handle, *const c_void);
type QueuePresent = unsafe extern "C" fn(Handle, *const c_void) -> VkResult;

/// Cabecera común de las estructuras de Vulkan (sType + pNext).
#[repr(C)]
struct BaseIn {
    s_type: i32,
    p_next: *const BaseIn,
}

#[repr(C)]
struct InstanceLink {
    next: *mut InstanceLink,
    gipa: Gipa,
    gpdpa: *const c_void,
}

#[repr(C)]
struct DeviceLink {
    next: *mut DeviceLink,
    gipa: Gipa,
    gdpa: Gdpa,
}

/// VkLayerInstanceCreateInfo / VkLayerDeviceCreateInfo (con la unión reducida a su puntero).
#[repr(C)]
struct LayerCreateInfo<L> {
    s_type: i32,
    p_next: *const BaseIn,
    function: i32,
    link: *mut L,
}

#[repr(C)]
pub struct NegotiateLayerInterface {
    s_type: i32,
    p_next: *mut c_void,
    loader_layer_interface_version: u32,
    gipa: Option<Gipa>,
    gdpa: Option<Gdpa>,
    gpdpa: *const c_void,
}

/// La tabla de despacho del cargador (primer puntero del objeto) identifica instancia y
/// dispositivo; las colas comparten la de su dispositivo.
unsafe fn key(h: Handle) -> usize {
    unsafe { *(h as *const usize) }
}

#[derive(Clone, Copy)]
struct Inst {
    key: usize,
    handle: Handle,
    gipa: Option<Gipa>,
    destroy: Option<DestroyInstance>,
}

#[derive(Clone, Copy)]
struct Dev {
    key: usize,
    gdpa: Option<Gdpa>,
    destroy: Option<DestroyDevice>,
    present: Option<QueuePresent>,
}

const N: usize = 64;
const NO_INST: Inst = Inst { key: 0, handle: core::ptr::null_mut(), gipa: None, destroy: None };
const NO_DEV: Dev = Dev { key: 0, gdpa: None, destroy: None, present: None };

/// Instancias y dispositivos vivos (tablas fijas: un juego tiene uno o dos de cada).
struct Registry {
    lock: AtomicBool,
    insts: core::cell::UnsafeCell<[Inst; N]>,
    devs: core::cell::UnsafeCell<[Dev; N]>,
}

// SAFETY: todo acceso a las tablas pasa por `with`, protegido por el cerrojo
unsafe impl Sync for Registry {}

static REG: Registry = Registry { lock: AtomicBool::new(false), insts: core::cell::UnsafeCell::new([NO_INST; N]), devs: core::cell::UnsafeCell::new([NO_DEV; N]) };

impl Registry {
    fn with<R>(&self, f: impl FnOnce(&mut [Inst; N], &mut [Dev; N]) -> R) -> R {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        // SAFETY: tenemos el cerrojo
        let r = unsafe { f(&mut *self.insts.get(), &mut *self.devs.get()) };
        self.lock.store(false, Ordering::Release);
        r
    }
}

fn find_inst(k: usize) -> Option<Inst> {
    REG.with(|i, _| i.iter().find(|x| x.key == k).copied())
}

fn find_dev(k: usize) -> Option<Dev> {
    REG.with(|_, d| d.iter().find(|x| x.key == k).copied())
}

// ── registro de fotogramas (estado compartido con la precarga de OpenGL si está cargada) ──

static OWN_STATE: State = State::new();
static SHARED: AtomicPtr<State> = AtomicPtr::new(core::ptr::null_mut());

fn state() -> &'static State {
    let mut p = SHARED.load(Ordering::Acquire);
    if p.is_null() {
        type Get = unsafe extern "C" fn() -> *const State;
        // SAFETY: búsqueda en el ámbito global; si la precarga de OpenGL está, devuelve su estado
        let f = unsafe { libc::dlsym(libc::RTLD_DEFAULT, SHARED_SYMBOL.as_ptr() as *const c_char) };
        let s = if f.is_null() { &OWN_STATE as *const State } else { unsafe { core::mem::transmute::<*mut c_void, Get>(f)() } };
        p = s as *mut State;
        SHARED.store(p, Ordering::Release);
    }
    // SAFETY: apunta a un `State` estático que vive lo que el proceso
    unsafe { &*p }
}

// ── funciones interceptadas ──

unsafe fn find_link<L>(mut p: *const BaseIn, s_type: i32) -> *mut LayerCreateInfo<L> {
    unsafe {
        while !p.is_null() {
            let li = p as *mut LayerCreateInfo<L>;
            if (*p).s_type == s_type && (*li).function == LAYER_LINK_INFO {
                return li;
            }
            p = (*p).p_next;
        }
    }
    core::ptr::null_mut()
}

unsafe extern "C" fn nt_CreateInstance(ci: *const BaseIn, alloc: *const c_void, out: *mut Handle) -> VkResult {
    unsafe {
        let link = find_link::<InstanceLink>((*ci).p_next, LOADER_INSTANCE_CREATE_INFO);
        if link.is_null() || (*link).link.is_null() {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        let gipa = (*(*link).link).gipa;
        (*link).link = (*(*link).link).next;
        let Some(create) = gipa(core::ptr::null_mut(), c"vkCreateInstance".as_ptr()) else { return VK_ERROR_INITIALIZATION_FAILED };
        let create: CreateInstance = core::mem::transmute(create);
        let r = create(ci, alloc, out);
        if r != VK_SUCCESS {
            return r;
        }
        let destroy = gipa(*out, c"vkDestroyInstance".as_ptr()).map(|f| core::mem::transmute::<unsafe extern "C" fn(), DestroyInstance>(f));
        let rec = Inst { key: key(*out), handle: *out, gipa: Some(gipa), destroy };
        REG.with(|i, _| {
            if let Some(slot) = i.iter_mut().find(|x| x.key == 0) {
                *slot = rec;
            }
        });
        r
    }
}

unsafe extern "C" fn nt_DestroyInstance(inst: Handle, alloc: *const c_void) {
    if inst.is_null() {
        return;
    }
    unsafe {
        let k = key(inst);
        let destroy = REG.with(|i, _| {
            let slot = i.iter_mut().find(|x| x.key == k)?;
            let d = slot.destroy;
            *slot = NO_INST;
            d
        });
        if let Some(d) = destroy {
            d(inst, alloc);
        }
    }
}

unsafe extern "C" fn nt_CreateDevice(phys: Handle, ci: *const BaseIn, alloc: *const c_void, out: *mut Handle) -> VkResult {
    unsafe {
        let link = find_link::<DeviceLink>((*ci).p_next, LOADER_DEVICE_CREATE_INFO);
        if link.is_null() || (*link).link.is_null() {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        let gipa = (*(*link).link).gipa;
        let gdpa = (*(*link).link).gdpa;
        (*link).link = (*(*link).link).next;
        let inst = find_inst(key(phys)).map(|i| i.handle).unwrap_or(core::ptr::null_mut());
        let Some(create) = gipa(inst, c"vkCreateDevice".as_ptr()) else { return VK_ERROR_INITIALIZATION_FAILED };
        let create: CreateDevice = core::mem::transmute(create);
        let r = create(phys, ci, alloc, out);
        if r != VK_SUCCESS {
            return r;
        }
        let destroy = gdpa(*out, c"vkDestroyDevice".as_ptr()).map(|f| core::mem::transmute::<unsafe extern "C" fn(), DestroyDevice>(f));
        let present = gdpa(*out, c"vkQueuePresentKHR".as_ptr()).map(|f| core::mem::transmute::<unsafe extern "C" fn(), QueuePresent>(f));
        let rec = Dev { key: key(*out), gdpa: Some(gdpa), destroy, present };
        REG.with(|_, d| {
            if let Some(slot) = d.iter_mut().find(|x| x.key == 0) {
                *slot = rec;
            }
        });
        r
    }
}

unsafe extern "C" fn nt_DestroyDevice(dev: Handle, alloc: *const c_void) {
    if dev.is_null() {
        return;
    }
    unsafe {
        let k = key(dev);
        let destroy = REG.with(|_, d| {
            let slot = d.iter_mut().find(|x| x.key == k)?;
            let f = slot.destroy;
            *slot = NO_DEV;
            f
        });
        if let Some(f) = destroy {
            f(dev, alloc);
        }
    }
}

unsafe extern "C" fn nt_QueuePresentKHR(queue: Handle, info: *const c_void) -> VkResult {
    unsafe {
        let Some(present) = find_dev(key(queue)).and_then(|d| d.present) else { return VK_ERROR_DEVICE_LOST };
        neontop_fps_shm::frame(state(), API_VULKAN);
        present(queue, info)
    }
}

fn own(name: &[u8]) -> VoidFn {
    // SAFETY: se convierten punteros a funciones de la capa al tipo genérico de Vulkan
    unsafe {
        let f: *const c_void = match name {
            b"vkCreateInstance" => nt_CreateInstance as *const c_void,
            b"vkDestroyInstance" => nt_DestroyInstance as *const c_void,
            b"vkCreateDevice" => nt_CreateDevice as *const c_void,
            b"vkDestroyDevice" => nt_DestroyDevice as *const c_void,
            b"vkQueuePresentKHR" => nt_QueuePresentKHR as *const c_void,
            _ => return None,
        };
        Some(core::mem::transmute::<*const c_void, unsafe extern "C" fn()>(f))
    }
}

unsafe extern "C" fn nt_GetDeviceProcAddr(dev: Handle, name: *const c_char) -> VoidFn {
    unsafe {
        let n = CStr::from_ptr(name).to_bytes();
        if n == b"vkGetDeviceProcAddr" {
            return Some(core::mem::transmute::<Gdpa, unsafe extern "C" fn()>(nt_GetDeviceProcAddr));
        }
        if n == b"vkQueuePresentKHR" || n == b"vkDestroyDevice" {
            return own(n);
        }
        if dev.is_null() {
            return None;
        }
        find_dev(key(dev)).and_then(|d| d.gdpa).and_then(|g| g(dev, name))
    }
}

unsafe extern "C" fn nt_GetInstanceProcAddr(inst: Handle, name: *const c_char) -> VoidFn {
    unsafe {
        let n = CStr::from_ptr(name).to_bytes();
        if n == b"vkGetInstanceProcAddr" {
            return Some(core::mem::transmute::<Gipa, unsafe extern "C" fn()>(nt_GetInstanceProcAddr));
        }
        if n == b"vkGetDeviceProcAddr" {
            return Some(core::mem::transmute::<Gdpa, unsafe extern "C" fn()>(nt_GetDeviceProcAddr));
        }
        if let Some(f) = own(n) {
            return Some(f);
        }
        if inst.is_null() {
            return None;
        }
        find_inst(key(inst)).and_then(|i| i.gipa).and_then(|g| g(inst, name))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vkNegotiateLoaderLayerInterfaceVersion(v: *mut NegotiateLayerInterface) -> VkResult {
    unsafe {
        if (*v).loader_layer_interface_version > 2 {
            (*v).loader_layer_interface_version = 2;
        }
        (*v).gipa = Some(nt_GetInstanceProcAddr);
        (*v).gdpa = Some(nt_GetDeviceProcAddr);
        (*v).gpdpa = core::ptr::null();
    }
    VK_SUCCESS
}

/// Por compatibilidad con cargadores antiguos que buscan los símbolos directamente.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vkGetInstanceProcAddr(inst: Handle, name: *const c_char) -> VoidFn {
    unsafe { nt_GetInstanceProcAddr(inst, name) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vkGetDeviceProcAddr(dev: Handle, name: *const c_char) -> VoidFn {
    unsafe { nt_GetDeviceProcAddr(dev, name) }
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
    fn layout() {
        // los punteros de la unión van justo después de sType, pNext y function
        assert_eq!(core::mem::offset_of!(LayerCreateInfo<InstanceLink>, link), 3 * core::mem::size_of::<usize>());
        assert_eq!(core::mem::offset_of!(NegotiateLayerInterface, gipa), 3 * core::mem::size_of::<usize>());
    }

    #[test]
    fn own_functions() {
        for n in [b"vkCreateInstance".as_slice(), b"vkQueuePresentKHR", b"vkDestroyDevice"] {
            assert!(own(n).is_some());
        }
        assert!(own(b"vkCmdDraw").is_none());
    }

    #[test]
    fn negotiate() {
        let mut v = NegotiateLayerInterface { s_type: 1, p_next: core::ptr::null_mut(), loader_layer_interface_version: 5, gipa: None, gdpa: None, gpdpa: core::ptr::null() };
        // SAFETY: estructura válida
        assert_eq!(unsafe { vkNegotiateLoaderLayerInterfaceVersion(&mut v) }, VK_SUCCESS);
        assert_eq!(v.loader_layer_interface_version, 2);
        assert!(v.gipa.is_some() && v.gdpa.is_some());
    }
}
