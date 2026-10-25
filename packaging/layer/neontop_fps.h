/*
 * NeonTop FPS: formato de la memoria compartida entre el juego y NeonTop.
 *
 * Cada proceso con la capa activa crea /dev/shm/neontop-fps-<pid> y anota en un anillo el
 * instante (CLOCK_MONOTONIC, ns) de cada fotograma presentado. NeonTop lee el anillo y calcula
 * FPS, tiempos de fotograma, 1 % / 0,1 % bajos, tirones, etc. El coste en el juego es una
 * lectura del reloj y dos escrituras en memoria por fotograma.
 *
 * El formato debe coincidir con src-tauri/src/collectors/games.rs.
 */
#ifndef NEONTOP_FPS_H
#define NEONTOP_FPS_H

#include <stdint.h>

#define NT_MAGIC 0x3153504654504e4eULL /* "NNPTFPS1" */
#define NT_VERSION 1
#define NT_CAP 8192 /* fotogramas en el anillo (~27 s a 300 FPS) */

enum nt_api { NT_API_VULKAN = 1, NT_API_GLX = 2, NT_API_EGL = 3 };

struct nt_shm {
    uint64_t magic;
    uint32_t version;
    uint32_t pid;
    uint32_t api;
    uint32_t capacity;
    uint32_t steam_app_id; /* SteamAppId / SteamGameId, 0 si no es de Steam */
    uint32_t bits;         /* 32 o 64 */
    char name[64];         /* ejecutable (sin ruta) */
    uint64_t start_ns;
    uint64_t count;        /* fotogramas totales; ts[count % capacity] es el siguiente hueco */
    uint64_t ts[NT_CAP];
};

void nt_frame(enum nt_api api);

#endif
