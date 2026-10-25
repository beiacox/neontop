/*
 * NeonTop FPS · capa Vulkan implícita. Solo se activa con NEONTOP_FPS=1 (ver el manifiesto)
 * y solo intercepta vkQueuePresentKHR para anotar cuándo se presenta cada fotograma; todo lo
 * demás pasa directamente a la siguiente capa o al driver. Cubre los juegos Vulkan nativos y
 * los de Proton/Wine (DXVK y VKD3D traducen DirectX a Vulkan).
 */
#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#include <vulkan/vk_layer.h>
#include <vulkan/vulkan.h>

#include "neontop_fps.h"

#define EXPORT __attribute__((visibility("default")))

/* La tabla de despacho del cargador (primer puntero del objeto) identifica instancia/dispositivo;
 * las colas comparten la de su dispositivo. */
static inline void *key(const void *h) { return *(void **)h; }

struct inst {
    void *key;
    VkInstance handle;
    PFN_vkGetInstanceProcAddr gipa;
    PFN_vkDestroyInstance destroy;
    struct inst *next;
};
struct dev {
    void *key;
    PFN_vkGetDeviceProcAddr gdpa;
    PFN_vkDestroyDevice destroy;
    PFN_vkQueuePresentKHR present;
    struct dev *next;
};

static struct inst *insts;
static struct dev *devs;
static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;

static struct inst *find_inst(void *k) {
    struct inst *i;
    pthread_mutex_lock(&mu);
    for (i = insts; i && i->key != k; i = i->next) {}
    pthread_mutex_unlock(&mu);
    return i;
}
static struct dev *find_dev(void *k) {
    struct dev *d;
    pthread_mutex_lock(&mu);
    for (d = devs; d && d->key != k; d = d->next) {}
    pthread_mutex_unlock(&mu);
    return d;
}

static VKAPI_ATTR VkResult VKAPI_CALL nt_CreateInstance(const VkInstanceCreateInfo *ci, const VkAllocationCallbacks *alloc, VkInstance *out) {
    VkLayerInstanceCreateInfo *link = (VkLayerInstanceCreateInfo *)ci->pNext;
    while (link && !(link->sType == VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO && link->function == VK_LAYER_LINK_INFO))
        link = (VkLayerInstanceCreateInfo *)link->pNext;
    if (!link) return VK_ERROR_INITIALIZATION_FAILED;
    PFN_vkGetInstanceProcAddr gipa = link->u.pLayerInfo->pfnNextGetInstanceProcAddr;
    link->u.pLayerInfo = link->u.pLayerInfo->pNext;
    PFN_vkCreateInstance create = (PFN_vkCreateInstance)gipa(VK_NULL_HANDLE, "vkCreateInstance");
    if (!create) return VK_ERROR_INITIALIZATION_FAILED;
    VkResult r = create(ci, alloc, out);
    if (r != VK_SUCCESS) return r;
    struct inst *i = calloc(1, sizeof *i);
    if (i) {
        i->key = key(*out);
        i->handle = *out;
        i->gipa = gipa;
        i->destroy = (PFN_vkDestroyInstance)gipa(*out, "vkDestroyInstance");
        pthread_mutex_lock(&mu);
        i->next = insts;
        insts = i;
        pthread_mutex_unlock(&mu);
    }
    return r;
}

static VKAPI_ATTR void VKAPI_CALL nt_DestroyInstance(VkInstance inst, const VkAllocationCallbacks *alloc) {
    if (!inst) return;
    void *k = key(inst);
    PFN_vkDestroyInstance destroy = NULL;
    pthread_mutex_lock(&mu);
    for (struct inst **p = &insts; *p; p = &(*p)->next) {
        if ((*p)->key == k) {
            struct inst *i = *p;
            destroy = i->destroy;
            *p = i->next;
            free(i);
            break;
        }
    }
    pthread_mutex_unlock(&mu);
    if (destroy) destroy(inst, alloc);
}

static VKAPI_ATTR VkResult VKAPI_CALL nt_CreateDevice(VkPhysicalDevice phys, const VkDeviceCreateInfo *ci, const VkAllocationCallbacks *alloc, VkDevice *out) {
    VkLayerDeviceCreateInfo *link = (VkLayerDeviceCreateInfo *)ci->pNext;
    while (link && !(link->sType == VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO && link->function == VK_LAYER_LINK_INFO))
        link = (VkLayerDeviceCreateInfo *)link->pNext;
    if (!link) return VK_ERROR_INITIALIZATION_FAILED;
    PFN_vkGetInstanceProcAddr gipa = link->u.pLayerInfo->pfnNextGetInstanceProcAddr;
    PFN_vkGetDeviceProcAddr gdpa = link->u.pLayerInfo->pfnNextGetDeviceProcAddr;
    link->u.pLayerInfo = link->u.pLayerInfo->pNext;
    struct inst *i = find_inst(key(phys));
    PFN_vkCreateDevice create = (PFN_vkCreateDevice)gipa(i ? i->handle : VK_NULL_HANDLE, "vkCreateDevice");
    if (!create) return VK_ERROR_INITIALIZATION_FAILED;
    VkResult r = create(phys, ci, alloc, out);
    if (r != VK_SUCCESS) return r;
    struct dev *d = calloc(1, sizeof *d);
    if (d) {
        d->key = key(*out);
        d->gdpa = gdpa;
        d->destroy = (PFN_vkDestroyDevice)gdpa(*out, "vkDestroyDevice");
        d->present = (PFN_vkQueuePresentKHR)gdpa(*out, "vkQueuePresentKHR");
        pthread_mutex_lock(&mu);
        d->next = devs;
        devs = d;
        pthread_mutex_unlock(&mu);
    }
    return r;
}

static VKAPI_ATTR void VKAPI_CALL nt_DestroyDevice(VkDevice dev, const VkAllocationCallbacks *alloc) {
    if (!dev) return;
    void *k = key(dev);
    PFN_vkDestroyDevice destroy = NULL;
    pthread_mutex_lock(&mu);
    for (struct dev **p = &devs; *p; p = &(*p)->next) {
        if ((*p)->key == k) {
            struct dev *d = *p;
            destroy = d->destroy;
            *p = d->next;
            free(d);
            break;
        }
    }
    pthread_mutex_unlock(&mu);
    if (destroy) destroy(dev, alloc);
}

static VKAPI_ATTR VkResult VKAPI_CALL nt_QueuePresentKHR(VkQueue queue, const VkPresentInfoKHR *info) {
    struct dev *d = find_dev(key(queue));
    if (!d || !d->present) return VK_ERROR_DEVICE_LOST;
    nt_frame(NT_API_VULKAN);
    return d->present(queue, info);
}

static PFN_vkVoidFunction own(const char *name) {
#define F(n) if (!strcmp(name, "vk" #n)) return (PFN_vkVoidFunction)nt_##n;
    F(CreateInstance) F(DestroyInstance) F(CreateDevice) F(DestroyDevice) F(QueuePresentKHR)
#undef F
    return NULL;
}

static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL nt_GetDeviceProcAddr(VkDevice dev, const char *name);
static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL nt_GetInstanceProcAddr(VkInstance inst, const char *name);

static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL nt_GetDeviceProcAddr(VkDevice dev, const char *name) {
    if (!strcmp(name, "vkGetDeviceProcAddr")) return (PFN_vkVoidFunction)nt_GetDeviceProcAddr;
    if (!strcmp(name, "vkQueuePresentKHR") || !strcmp(name, "vkDestroyDevice")) return own(name);
    struct dev *d = dev ? find_dev(key(dev)) : NULL;
    return d ? d->gdpa(dev, name) : NULL;
}

static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL nt_GetInstanceProcAddr(VkInstance inst, const char *name) {
    if (!strcmp(name, "vkGetInstanceProcAddr")) return (PFN_vkVoidFunction)nt_GetInstanceProcAddr;
    if (!strcmp(name, "vkGetDeviceProcAddr")) return (PFN_vkVoidFunction)nt_GetDeviceProcAddr;
    PFN_vkVoidFunction f = own(name);
    if (f) return f;
    struct inst *i = inst ? find_inst(key(inst)) : NULL;
    return i ? i->gipa(inst, name) : NULL;
}

EXPORT VKAPI_ATTR VkResult VKAPI_CALL vkNegotiateLoaderLayerInterfaceVersion(VkNegotiateLayerInterface *v) {
    if (v->loaderLayerInterfaceVersion > 2) v->loaderLayerInterfaceVersion = 2;
    v->pfnGetInstanceProcAddr = nt_GetInstanceProcAddr;
    v->pfnGetDeviceProcAddr = nt_GetDeviceProcAddr;
    v->pfnGetPhysicalDeviceProcAddr = NULL;
    return VK_SUCCESS;
}

/* por compatibilidad con cargadores antiguos que buscan los símbolos directamente */
EXPORT VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL vkGetInstanceProcAddr(VkInstance inst, const char *name) { return nt_GetInstanceProcAddr(inst, name); }
EXPORT VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL vkGetDeviceProcAddr(VkDevice dev, const char *name) { return nt_GetDeviceProcAddr(dev, name); }
