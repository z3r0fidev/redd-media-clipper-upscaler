// What ncnn's C API does not expose: the Vulkan instance, device listing, device type and heap
// budget. Plain C functions over ncnn's C++ API; no exceptions cross this boundary.
#include <ncnn/gpu.h>
#include <cstring>

extern "C" {

int rsr_gpu_init() { return ncnn::create_gpu_instance(); }

void rsr_gpu_destroy() { ncnn::destroy_gpu_instance(); }

int rsr_gpu_count() { return ncnn::get_gpu_count(); }

// Fills the device's name (NUL-terminated, truncated to name_len), its type (0 discrete,
// 1 integrated, 2 virtual, 3 cpu), fp16 storage support and heap budget in MiB. Returns 0, or -1
// for an index out of range.
int rsr_gpu_info(int index, char* name, int name_len, int* type, int* fp16, unsigned* heap_mb) {
    if (index < 0 || index >= ncnn::get_gpu_count() || name_len < 1) return -1;
    const ncnn::GpuInfo& info = ncnn::get_gpu_info(index);
    std::strncpy(name, info.device_name(), (size_t)name_len - 1);
    name[name_len - 1] = '\0';
    *type = info.type();
    *fp16 = info.support_fp16_storage() ? 1 : 0;
    ncnn::VulkanDevice* dev = ncnn::get_gpu_device(index);
    *heap_mb = dev ? dev->get_heap_budget() : 0;
    return 0;
}

}
