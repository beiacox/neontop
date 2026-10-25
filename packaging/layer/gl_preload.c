/*
 * NeonTop FPS · OpenGL (GLX y EGL) mediante LD_PRELOAD (lo carga el lanzador `neontop-fps`).
 *
 * Intercepta glXSwapBuffers / eglSwapBuffers para anotar cada fotograma. Muchos juegos (SDL,
 * motores propios) no enlazan esas funciones sino que las piden con dlsym() o
 * glXGetProcAddress(), así que también se interceptan esas búsquedas y se devuelve el
 * envoltorio. Todo lo demás pasa intacto.
 *
 * Convivencia con otras precargas que también interceptan dlsym (overlay de Steam, MangoHud):
 *  - Las búsquedas con RTLD_NEXT se resuelven respecto a QUIEN LLAMA, no respecto a esta
 *    librería. Si se reenviaran tal cual, glibc vería esta librería como llamante y devolvería
 *    al otro interceptor su propia función: el overlay de Steam se llamaba a sí mismo sin fin
 *    (100 % de CPU en steam-launch-wrapper, el juego no llegaba a arrancar).
 *  - Si este dlsym vuelve a entrar en el mismo hilo (otro interceptor llamando al dlsym global),
 *    va directo al de glibc: ningún ciclo entre interceptores es posible.
 *  - En los procesos de la cadena de lanzamiento (steam-launch-wrapper, reaper, pressure-vessel,
 *    Proton, wineserver…) no se intercepta nada: solo se deja pasar.
 * No hay constructor: todo se resuelve la primera vez que hace falta, sin esperar a nada.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <link.h>
#include <string.h>

#include "neontop_fps.h"

#define EXPORT __attribute__((visibility("default")))

typedef void (*glx_swap_fn)(void *dpy, unsigned long drawable);
typedef unsigned (*egl_swap_fn)(void *dpy, void *surface);
typedef void *(*getproc_fn)(const char *);
typedef void *(*dlsym_fn)(void *, const char *);

static glx_swap_fn real_glx_swap;
static egl_swap_fn real_egl_swap;
static getproc_fn real_glx_getproc, real_glx_getproc_arb, real_egl_getproc;

/* Profundidad de llamadas a nuestro dlsym en este hilo (para cortar ciclos). */
static __thread int depth;

static dlsym_fn versioned(void *handle) {
    dlsym_fn f = (dlsym_fn)dlvsym(handle, "dlsym", "GLIBC_2.34");
    if (!f) f = (dlsym_fn)dlvsym(handle, "dlsym", sizeof(void *) == 8 ? "GLIBC_2.2.5" : "GLIBC_2.0");
    return f;
}

/*
 * Busca `name` en la tabla de símbolos dinámicos de un objeto ya cargado (tabla hash GNU), sin
 * llamar a dlsym/dlvsym/dlopen: otros interceptores (el overlay de Steam intercepta dlsym y dlopen)
 * no pueden meterse en medio. Es lo que hace MangoHud con elfhacks.
 */
static void *elf_lookup(struct link_map *l, const char *name) {
    const ElfW(Sym) *symtab = NULL;
    const char *strtab = NULL;
    const uint32_t *gnu = NULL;
    const ElfW(Half) *versym = NULL;
    if (!l || !l->l_ld) return NULL;
    // en glibc las direcciones de la sección dinámica ya suelen estar reubicadas; si no, se suma la base
#define PTR(v) ((v) < (ElfW(Addr))l->l_addr ? (v) + (ElfW(Addr))l->l_addr : (v))
    for (const ElfW(Dyn) *d = l->l_ld; d && d->d_tag != DT_NULL; d++) {
        if (d->d_tag == DT_SYMTAB) symtab = (const ElfW(Sym) *)PTR(d->d_un.d_ptr);
        else if (d->d_tag == DT_STRTAB) strtab = (const char *)PTR(d->d_un.d_ptr);
        else if (d->d_tag == DT_GNU_HASH) gnu = (const uint32_t *)PTR(d->d_un.d_ptr);
        else if (d->d_tag == DT_VERSYM) versym = (const ElfW(Half) *)PTR(d->d_un.d_ptr);
    }
#undef PTR
    if (!symtab || !strtab || !gnu) return NULL;
    uint32_t nbuckets = gnu[0], symoffset = gnu[1], bloom = gnu[2];
    const uint32_t *buckets = gnu + 4 + bloom * (sizeof(ElfW(Addr)) / 4);
    const uint32_t *chains = buckets + nbuckets;
    uint32_t h = 5381;
    for (const unsigned char *c = (const unsigned char *)name; *c; c++) h = h * 33 + *c;
    if (!nbuckets) return NULL;
    uint32_t i = buckets[h % nbuckets];
    if (i < symoffset) return NULL;
    // con varias versiones del símbolo se prefiere la de por defecto (las antiguas llevan el bit 0x8000)
    void *hidden = NULL;
    for (;; i++) {
        uint32_t ch = chains[i - symoffset];
        const ElfW(Sym) *sym = &symtab[i];
        if ((h | 1) == (ch | 1) && sym->st_shndx != SHN_UNDEF && !strcmp(strtab + sym->st_name, name)) {
            void *addr = (void *)(l->l_addr + sym->st_value);
            // IFUNC (memcpy, strlen…): la entrada es el resolutor; dlsym devolvería lo que este elige
            if (ELF64_ST_TYPE(sym->st_info) == STT_GNU_IFUNC) addr = ((void *(*)(void))addr)();
            if (!versym || !(versym[i] & 0x8000)) return addr;
            if (!hidden) hidden = addr;
        }
        if (ch & 1) break;
    }
    return hidden;
}

/* dlsym de glibc, sacado de la tabla de símbolos de libc (no "el siguiente", que podría ser otro interceptor). */
static dlsym_fn libc_dlsym(void) {
    static dlsym_fn f;
    if (!f) {
        for (struct link_map *l = _r_debug.r_map; l && !f; l = l->l_next) {
            const char *n = l->l_name ? strrchr(l->l_name, '/') : NULL;
            if (n && !strncmp(n + 1, "libc.so.6", 9)) f = (dlsym_fn)elf_lookup(l, "dlsym");
        }
        if (!f) f = versioned(RTLD_NEXT);
    }
    return f;
}

/* dlsym siguiente en la cadena de precargas (otro interceptor o glibc). */
static dlsym_fn chain_dlsym(void) {
    static dlsym_fn f;
    if (!f) f = versioned(RTLD_NEXT);
    if (!f) f = libc_dlsym();
    return f;
}

/* Símbolo siguiente respecto a ESTA librería (para encontrar el glXSwapBuffers real). */
static void *next_sym(const char *name) {
    dlsym_fn d = libc_dlsym();
    return d ? d(RTLD_NEXT, name) : NULL;
}

/*
 * RTLD_NEXT respecto a `caller`: el primer objeto cargado después del que contiene `caller` que
 * defina `name`. En glibc el handle de un objeto es su link_map.
 */
static void *next_after(const void *caller, const char *name) {
    Dl_info info;
    struct link_map *lm = NULL;
    if (!dladdr1(caller, &info, (void **)&lm, RTLD_DL_LINKMAP) || !lm) {
        dlsym_fn d = libc_dlsym();
        return d ? d(RTLD_NEXT, name) : NULL;
    }
    // búsqueda directa en la tabla de cada objeto posterior (no dlsym: los objetos del arranque y el
    // vDSO no sirven como handle, y así ningún interceptor se mete en medio)
    for (struct link_map *l = lm->l_next; l; l = l->l_next) {
        void *r = elf_lookup(l, name);
        if (r) return r;
    }
    return NULL;
}

/*
 * ¿Es un proceso de la cadena de lanzamiento (no el juego)? Se decide una vez, sin llamar a nada:
 * el nombre sale de program_invocation_short_name (glibc, ya en memoria). Leer /proc con open()
 * no vale: el overlay de Steam intercepta open() y su open() llama a dlsym → recursión.
 */
static int passive(void) {
    static int state = -1;
    if (state >= 0) return state;
    state = 1; // provisional: si algo vuelve a entrar mientras se decide, no intercepta
    const char *name = program_invocation_short_name ? program_invocation_short_name : "";
    static const char *const skip[] = {"steam-launch-wrapper", "reaper", "bwrap", "srt-bwrap", "wineserver", "steam", "steamwebhelper",
                                       "timeout", "sh", "bash", "zsh", "dash", "env", NULL};
    static const char *const prefix[] = {"pv-", "srt-", "steam-runtime", "python", "proton", "gamemode", "mangohud", NULL};
    int p = 0;
    for (const char *const *s = skip; *s; s++)
        if (!strcmp(name, *s)) p = 1;
    for (const char *const *x = prefix; *x; x++)
        if (!strncmp(name, *x, strlen(*x))) p = 1;
    state = p;
    return state;
}

EXPORT void glXSwapBuffers(void *dpy, unsigned long drawable) {
    if (!real_glx_swap) real_glx_swap = (glx_swap_fn)next_sym("glXSwapBuffers");
    if (!passive()) nt_frame(NT_API_GLX);
    if (real_glx_swap) real_glx_swap(dpy, drawable);
}

EXPORT unsigned eglSwapBuffers(void *dpy, void *surface) {
    if (!real_egl_swap) real_egl_swap = (egl_swap_fn)next_sym("eglSwapBuffers");
    if (!passive()) nt_frame(NT_API_EGL);
    return real_egl_swap ? real_egl_swap(dpy, surface) : 0;
}

/* Devuelve nuestro envoltorio si `name` es una función que interceptamos. */
static void *wrap(const char *name, void *real) {
    if (!name || !real || passive()) return real;
    // la búsqueda puede devolver nuestra propia función: no es la real
    if (real == (void *)glXSwapBuffers || real == (void *)eglSwapBuffers) return real;
    if (!strcmp(name, "glXSwapBuffers")) {
        real_glx_swap = (glx_swap_fn)real;
        return (void *)glXSwapBuffers;
    }
    if (!strcmp(name, "eglSwapBuffers")) {
        real_egl_swap = (egl_swap_fn)real;
        return (void *)eglSwapBuffers;
    }
    return real;
}

EXPORT void *glXGetProcAddress(const char *name) {
    if (!real_glx_getproc) real_glx_getproc = (getproc_fn)next_sym("glXGetProcAddress");
    return wrap(name, real_glx_getproc ? real_glx_getproc(name) : NULL);
}

EXPORT void *glXGetProcAddressARB(const char *name) {
    if (!real_glx_getproc_arb) real_glx_getproc_arb = (getproc_fn)next_sym("glXGetProcAddressARB");
    return wrap(name, real_glx_getproc_arb ? real_glx_getproc_arb(name) : NULL);
}

EXPORT void *eglGetProcAddress(const char *name) {
    if (!real_egl_getproc) real_egl_getproc = (getproc_fn)next_sym("eglGetProcAddress");
    return wrap(name, real_egl_getproc ? real_egl_getproc(name) : NULL);
}

EXPORT void *dlsym(void *handle, const char *name) {
    void *r;
    if (handle == RTLD_NEXT) {
        // "el siguiente después de mí" se resuelve respecto al que llama, no respecto a nosotros
        r = next_after(__builtin_return_address(0), name);
    } else {
        dlsym_fn d = ++depth > 1 ? libc_dlsym() : chain_dlsym();
        r = d ? d(handle, name) : NULL;
        depth--;
    }
    if (!r || passive()) return r;
    // las búsquedas de *GetProcAddress también se redirigen, para envolver lo que devuelvan
    if (r == (void *)glXGetProcAddress || r == (void *)glXGetProcAddressARB || r == (void *)eglGetProcAddress) return r;
    if (!strcmp(name, "glXGetProcAddress")) {
        real_glx_getproc = (getproc_fn)r;
        return (void *)glXGetProcAddress;
    }
    if (!strcmp(name, "glXGetProcAddressARB")) {
        real_glx_getproc_arb = (getproc_fn)r;
        return (void *)glXGetProcAddressARB;
    }
    if (!strcmp(name, "eglGetProcAddress")) {
        real_egl_getproc = (getproc_fn)r;
        return (void *)eglGetProcAddress;
    }
    return wrap(name, r);
}
