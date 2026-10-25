/* NeonTop FPS: creación del segmento compartido y registro de fotogramas. */
#define _GNU_SOURCE
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

#include "neontop_fps.h"

/*
 * Estado con visibilidad por defecto (no static): si un proceso carga la capa Vulkan y la
 * librería de OpenGL, el enlazador dinámico une ambas copias al mismo estado. Así hay un solo
 * segmento por proceso y, con OpenGL sobre Vulkan (Zink), no se cuenta cada fotograma dos
 * veces: solo cuenta la primera API que presenta.
 */
#define shm neontop_fps_shm
#define failed neontop_fps_failed
#define path neontop_fps_path
#define lock neontop_fps_lock
#define SHARED __attribute__((visibility("default")))
SHARED struct nt_shm *shm;
SHARED int failed;
SHARED char path[64];
SHARED pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

static uint64_t now_ns(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)t.tv_sec * 1000000000ull + (uint64_t)t.tv_nsec;
}

/* Nombre del juego: argv[0] sin ruta (con Wine suele ser "C:\...\Juego.exe"). */
static void exe_name(char *out, size_t n) {
    char buf[512] = {0};
    int fd = open("/proc/self/cmdline", O_RDONLY | O_CLOEXEC);
    if (fd >= 0) {
        ssize_t r = read(fd, buf, sizeof buf - 1);
        close(fd);
        if (r > 0) {
            const char *b = buf;
            for (const char *p = buf; *p; p++)
                if (*p == '/' || *p == '\\') b = p + 1;
            snprintf(out, n, "%s", b);
            return;
        }
    }
    fd = open("/proc/self/comm", O_RDONLY | O_CLOEXEC);
    if (fd >= 0) {
        ssize_t r = read(fd, out, n - 1);
        close(fd);
        if (r > 0) out[r - (out[r - 1] == '\n')] = 0;
    }
}

static void cleanup(void) {
    if (path[0]) unlink(path);
}

static void open_shm(enum nt_api api) {
    snprintf(path, sizeof path, "/dev/shm/neontop-fps-%d", (int)getpid());
    int fd = open(path, O_RDWR | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
    if (fd < 0 || ftruncate(fd, sizeof(struct nt_shm)) != 0) {
        if (fd >= 0) close(fd);
        failed = 1;
        return;
    }
    void *m = mmap(NULL, sizeof(struct nt_shm), PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    close(fd);
    if (m == MAP_FAILED) {
        failed = 1;
        return;
    }
    struct nt_shm *s = m;
    s->version = NT_VERSION;
    s->pid = (uint32_t)getpid();
    s->api = api;
    s->capacity = NT_CAP;
    s->bits = sizeof(void *) * 8;
    const char *app = getenv("SteamAppId");
    if (!app || !*app) app = getenv("SteamGameId");
    s->steam_app_id = app ? (uint32_t)strtoul(app, NULL, 10) : 0;
    exe_name(s->name, sizeof s->name);
    s->start_ns = now_ns();
    /* el número mágico se escribe el último: NeonTop ignora segmentos a medio preparar */
    __atomic_store_n(&s->magic, NT_MAGIC, __ATOMIC_RELEASE);
    shm = s;
    atexit(cleanup);
}

SHARED void nt_frame(enum nt_api api) {
    if (!shm) {
        if (failed) return;
        pthread_mutex_lock(&lock);
        if (!shm && !failed) open_shm(api);
        pthread_mutex_unlock(&lock);
        if (!shm) return;
    }
    if (shm->api != (uint32_t)api) return;
    uint64_t c = __atomic_load_n(&shm->count, __ATOMIC_RELAXED);
    shm->ts[c % NT_CAP] = now_ns();
    __atomic_store_n(&shm->count, c + 1, __ATOMIC_RELEASE);
}
