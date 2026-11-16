/* Read-only Vulkan capability probe; build with aarch64 Linux zig cc and -ldl. */
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
#include <dlfcn.h>
#include <stdio.h>
int main(void) {
    void *lib = dlopen("libvulkan.so.1", RTLD_NOW);
    if (!lib) { puts(dlerror()); return 1; }
    PFN_vkGetInstanceProcAddr get = (PFN_vkGetInstanceProcAddr)dlsym(lib, "vkGetInstanceProcAddr");
    PFN_vkCreateInstance create = (PFN_vkCreateInstance)get(VK_NULL_HANDLE, "vkCreateInstance");
    VkApplicationInfo app = { .sType=VK_STRUCTURE_TYPE_APPLICATION_INFO, .apiVersion=VK_API_VERSION_1_0 };
    VkInstanceCreateInfo ci = { .sType=VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO, .pApplicationInfo=&app };
    VkInstance instance;
    if (create(&ci, NULL, &instance)) return 2;
    PFN_vkEnumeratePhysicalDevices devices = (PFN_vkEnumeratePhysicalDevices)get(instance, "vkEnumeratePhysicalDevices");
    PFN_vkGetPhysicalDeviceProperties properties = (PFN_vkGetPhysicalDeviceProperties)get(instance, "vkGetPhysicalDeviceProperties");
    uint32_t count=1;
    VkPhysicalDevice device;
    if (devices(instance, &count, &device) || !count) return 3;
    VkPhysicalDeviceProperties p;
    properties(device, &p);
    printf("%s: Vulkan %u.%u.%u; max 2D texture %u; storage buffers/stage %u; uniform range %u; storage range %u\n", p.deviceName,
        VK_VERSION_MAJOR(p.apiVersion), VK_VERSION_MINOR(p.apiVersion), VK_VERSION_PATCH(p.apiVersion),
        p.limits.maxImageDimension2D, p.limits.maxPerStageDescriptorStorageBuffers,
        p.limits.maxUniformBufferRange, p.limits.maxStorageBufferRange);
    ((PFN_vkDestroyInstance)get(instance, "vkDestroyInstance"))(instance, NULL);
    dlclose(lib);
    return 0;
}
