//! The only module with `unsafe`: ncnn's C API, the C++ shim (`shim.cpp`) and the stderr
//! redirection. Everything here is wrapped in owning types whose `Drop` frees the native object,
//! so the rest of the worker is safe Rust.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::io::Write;
use std::path::Path;

type NetPtr = *mut c_void;
type OptionPtr = *mut c_void;
type MatPtr = *mut c_void;
type ExtractorPtr = *mut c_void;

const PIXEL_RGB: c_int = 1;

unsafe extern "C" {
    fn ncnn_version() -> *const c_char;
    fn ncnn_net_create() -> NetPtr;
    fn ncnn_net_destroy(net: NetPtr);
    fn ncnn_net_get_option(net: NetPtr) -> OptionPtr;
    fn ncnn_net_set_vulkan_device(net: NetPtr, device_index: c_int);
    #[cfg(not(windows))]
    fn ncnn_net_load_param(net: NetPtr, path: *const c_char) -> c_int;
    #[cfg(not(windows))]
    fn ncnn_net_load_model(net: NetPtr, path: *const c_char) -> c_int;
    #[cfg(windows)]
    fn ncnn_net_load_param_w(net: NetPtr, path: *const u16) -> c_int;
    #[cfg(windows)]
    fn ncnn_net_load_model_w(net: NetPtr, path: *const u16) -> c_int;
    fn ncnn_option_set_use_vulkan_compute(opt: OptionPtr, enable: c_int);
    fn ncnn_option_set_num_threads(opt: OptionPtr, n: c_int);
    fn ncnn_option_set_use_fp16_packed(opt: OptionPtr, enable: c_int);
    fn ncnn_option_set_use_fp16_storage(opt: OptionPtr, enable: c_int);
    fn ncnn_option_set_use_fp16_arithmetic(opt: OptionPtr, enable: c_int);
    fn ncnn_option_set_use_int8_storage(opt: OptionPtr, enable: c_int);
    fn ncnn_option_set_use_winograd_convolution(opt: OptionPtr, enable: c_int);
    fn ncnn_mat_from_pixels_roi(
        pixels: *const u8,
        kind: c_int,
        w: c_int,
        h: c_int,
        stride: c_int,
        roix: c_int,
        roiy: c_int,
        roiw: c_int,
        roih: c_int,
        allocator: *mut c_void,
    ) -> MatPtr;
    fn ncnn_mat_to_pixels(mat: MatPtr, pixels: *mut u8, kind: c_int, stride: c_int);
    fn ncnn_mat_substract_mean_normalize(mat: MatPtr, mean: *const f32, norm: *const f32);
    fn ncnn_mat_get_w(mat: MatPtr) -> c_int;
    fn ncnn_mat_get_h(mat: MatPtr) -> c_int;
    fn ncnn_mat_get_c(mat: MatPtr) -> c_int;
    fn ncnn_mat_destroy(mat: MatPtr);
    fn ncnn_extractor_create(net: NetPtr) -> ExtractorPtr;
    fn ncnn_extractor_destroy(ex: ExtractorPtr);
    fn ncnn_extractor_input(ex: ExtractorPtr, name: *const c_char, mat: MatPtr) -> c_int;
    fn ncnn_extractor_extract(ex: ExtractorPtr, name: *const c_char, mat: *mut MatPtr) -> c_int;

    fn rsr_gpu_init() -> c_int;
    fn rsr_gpu_destroy();
    fn rsr_gpu_count() -> c_int;
    fn rsr_gpu_info(
        index: c_int,
        name: *mut c_char,
        name_len: c_int,
        kind: *mut c_int,
        fp16: *mut c_int,
        heap_mb: *mut c_uint,
    ) -> c_int;
}

/// ncnn's version string (e.g. `1.0.20260526`).
pub fn version() -> String {
    // SAFETY: ncnn_version returns a pointer to a static NUL-terminated string.
    unsafe { CStr::from_ptr(ncnn_version()) }.to_string_lossy().into_owned()
}

/// A Vulkan device as ncnn sees it.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub index: u32,
    pub name: String,
    /// ncnn's type: 0 discrete, 1 integrated, 2 virtual, 3 cpu.
    pub kind: i32,
    pub fp16: bool,
    pub heap_budget_mb: u32,
}

/// ncnn's Vulkan instance; created once per process and destroyed after every net that uses it
/// (owners keep it as their last field so it drops last).
#[derive(Debug)]
pub struct Gpu {
    _private: (),
}

impl Gpu {
    /// Creates the instance. `None` when no Vulkan loader or driver is available.
    pub fn init() -> Option<Gpu> {
        // SAFETY: plain C call; failure is reported by the return value.
        (unsafe { rsr_gpu_init() } == 0).then_some(Gpu { _private: () })
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        // SAFETY: the instance exists while `self` does.
        let n = unsafe { rsr_gpu_count() }.max(0);
        (0..n)
            .filter_map(|i| {
                let mut name = [0 as c_char; 256];
                let (mut kind, mut fp16, mut heap) = (0, 0, 0);
                // SAFETY: `name` is a writable buffer of the stated length; the shim writes a
                // NUL-terminated string into it and checks the index.
                let ok = unsafe {
                    rsr_gpu_info(i, name.as_mut_ptr(), name.len() as c_int, &mut kind, &mut fp16, &mut heap)
                };
                (ok == 0).then(|| DeviceInfo {
                    index: i as u32,
                    // SAFETY: NUL-terminated by the shim (the buffer is never empty).
                    name: unsafe { CStr::from_ptr(name.as_ptr()) }.to_string_lossy().into_owned(),
                    kind,
                    fp16: fp16 != 0,
                    heap_budget_mb: heap,
                })
            })
            .collect()
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        // SAFETY: every Net borrows the Gpu, so all nets are gone by now.
        unsafe { rsr_gpu_destroy() }
    }
}

/// Net options set before the model loads.
#[derive(Debug, Clone, Copy)]
pub struct NetOptions {
    pub device: u32,
    /// `None` keeps ncnn's choice for the device.
    pub fp16: Option<bool>,
    pub threads: u32,
    /// Winograd convolution (ncnn's default). Off is about twice as fast on Intel integrated GPUs
    /// (measured 2026-10-20, research R13).
    pub winograd: bool,
}

/// A loaded ncnn network on one Vulkan device.
#[derive(Debug)]
pub struct Net<'g> {
    ptr: NetPtr,
    _gpu: &'g Gpu,
}

impl<'g> Net<'g> {
    /// Loads the graph `param` (text) and the weights `bin` onto the device. Files, not memory:
    /// ncnn's file reader stops at the end of a truncated file, its memory reader does not.
    pub fn load(gpu: &'g Gpu, opts: NetOptions, param: &Path, bin: &Path) -> Result<Net<'g>, String> {
        let (param_c, bin_c) = (native_path(param)?, native_path(bin)?);
        // SAFETY: ncnn_net_create returns a fresh net (or null on allocation failure, checked).
        let ptr = unsafe { ncnn_net_create() };
        if ptr.is_null() {
            return Err("could not create the network".into());
        }
        let net = Net { ptr, _gpu: gpu };
        // SAFETY: `net.ptr` is valid; its option struct is set before anything loads; the paths
        // are NUL-terminated and outlive the calls.
        unsafe {
            let opt = ncnn_net_get_option(net.ptr);
            ncnn_option_set_use_vulkan_compute(opt, 1);
            ncnn_option_set_num_threads(opt, opts.threads.clamp(1, 64) as c_int);
            ncnn_option_set_use_int8_storage(opt, 0);
            if let Some(on) = opts.fp16 {
                ncnn_option_set_use_fp16_packed(opt, on.into());
                ncnn_option_set_use_fp16_storage(opt, on.into());
                ncnn_option_set_use_fp16_arithmetic(opt, on.into());
            }
            if !opts.winograd {
                ncnn_option_set_use_winograd_convolution(opt, 0);
            }
            ncnn_net_set_vulkan_device(net.ptr, opts.device as c_int);
            if load_param(net.ptr, &param_c) != 0 {
                return Err(format!("the model graph {} could not be loaded", param.display()));
            }
            if load_model(net.ptr, &bin_c) != 0 {
                return Err(format!("the model weights {} could not be loaded", bin.display()));
            }
        }
        Ok(net)
    }

    /// Runs the network on the `roi` (x, y, w, h) of a packed RGB frame of `size` (w, h) and
    /// returns the output as packed RGB with its size. Input is scaled to 0–1 and output back
    /// to 0–255, as the Real-ESRGAN models expect.
    pub fn run(
        &self,
        frame: &[u8],
        size: (u32, u32),
        roi: (u32, u32, u32, u32),
    ) -> Result<(Vec<u8>, (u32, u32)), String> {
        let (w, h) = size;
        let (x, y, rw, rh) = roi;
        assert!(frame.len() >= (w * h * 3) as usize && x + rw <= w && y + rh <= h, "roi inside the frame");
        let input = Mat(
            // SAFETY: `frame` holds w·h packed RGB pixels (asserted) and the roi lies inside it.
            unsafe {
                ncnn_mat_from_pixels_roi(
                    frame.as_ptr(),
                    PIXEL_RGB,
                    w as c_int,
                    h as c_int,
                    (w * 3) as c_int,
                    x as c_int,
                    y as c_int,
                    rw as c_int,
                    rh as c_int,
                    std::ptr::null_mut(),
                )
            },
        );
        if input.0.is_null() {
            return Err("out of memory for the input frame".into());
        }
        let to_unit = [1.0f32 / 255.0; 3];
        // SAFETY: valid mat; mean may be null (normalise only); norm has 3 values for 3 channels.
        unsafe { ncnn_mat_substract_mean_normalize(input.0, std::ptr::null(), to_unit.as_ptr()) };
        let ex = Extractor(
            // SAFETY: valid net.
            unsafe { ncnn_extractor_create(self.ptr) },
        );
        let mut out = Mat(std::ptr::null_mut());
        // SAFETY: valid extractor and mat; blob names are NUL-terminated; ncnn writes a new mat
        // (owned by us, freed by Mat's Drop) into `out.0`.
        let rc = unsafe {
            if ncnn_extractor_input(ex.0, c"data".as_ptr(), input.0) != 0 {
                -100
            } else {
                ncnn_extractor_extract(ex.0, c"output".as_ptr(), &mut out.0)
            }
        };
        if rc != 0 || out.0.is_null() {
            return Err(format!("the GPU could not run the model (ncnn error {rc})"));
        }
        // SAFETY: valid output mat.
        let (ow, oh, oc) = unsafe { (ncnn_mat_get_w(out.0), ncnn_mat_get_h(out.0), ncnn_mat_get_c(out.0)) };
        if ow <= 0 || oh <= 0 || oc != 3 {
            return Err(format!("the model returned an unexpected {ow}×{oh}×{oc} output"));
        }
        let to_byte = [255.0f32; 3];
        let mut pixels = vec![0u8; ow as usize * oh as usize * 3];
        // SAFETY: valid mat of ow×oh×3; `pixels` holds exactly ow·oh·3 bytes with stride ow·3.
        unsafe {
            ncnn_mat_substract_mean_normalize(out.0, std::ptr::null(), to_byte.as_ptr());
            ncnn_mat_to_pixels(out.0, pixels.as_mut_ptr(), PIXEL_RGB, ow * 3);
        }
        Ok((pixels, (ow as u32, oh as u32)))
    }
}

impl Drop for Net<'_> {
    fn drop(&mut self) {
        // SAFETY: created by ncnn_net_create and dropped once.
        unsafe { ncnn_net_destroy(self.ptr) }
    }
}

#[cfg(not(windows))]
fn native_path(p: &Path) -> Result<Vec<c_char>, String> {
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(p.as_os_str().as_bytes()).map_err(|_| format!("bad path {}", p.display()))?;
    Ok(c.as_bytes_with_nul().iter().map(|b| *b as c_char).collect())
}

#[cfg(windows)]
fn native_path(p: &Path) -> Result<Vec<u16>, String> {
    use std::os::windows::ffi::OsStrExt;
    let w: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
    if w[..w.len() - 1].contains(&0) {
        return Err(format!("bad path {}", p.display()));
    }
    Ok(w)
}

#[cfg(not(windows))]
unsafe fn load_param(net: NetPtr, path: &[c_char]) -> c_int {
    // SAFETY: the caller passes a valid net and a NUL-terminated path.
    unsafe { ncnn_net_load_param(net, path.as_ptr()) }
}

#[cfg(not(windows))]
unsafe fn load_model(net: NetPtr, path: &[c_char]) -> c_int {
    // SAFETY: as for load_param.
    unsafe { ncnn_net_load_model(net, path.as_ptr()) }
}

#[cfg(windows)]
unsafe fn load_param(net: NetPtr, path: &[u16]) -> c_int {
    // SAFETY: the caller passes a valid net and a NUL-terminated wide path.
    unsafe { ncnn_net_load_param_w(net, path.as_ptr()) }
}

#[cfg(windows)]
unsafe fn load_model(net: NetPtr, path: &[u16]) -> c_int {
    // SAFETY: as for load_param.
    unsafe { ncnn_net_load_model_w(net, path.as_ptr()) }
}

struct Mat(MatPtr);

impl Drop for Mat {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a mat returned by ncnn's C API, freed once.
            unsafe { ncnn_mat_destroy(self.0) }
        }
    }
}

struct Extractor(ExtractorPtr);

impl Drop for Extractor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: created by ncnn_extractor_create, freed once (before its net).
            unsafe { ncnn_extractor_destroy(self.0) }
        }
    }
}

/// Takes over stderr for the JSON protocol: returns a writer on a duplicate of the original
/// stderr and points file descriptor 2 at the null device, so ncnn's own diagnostics (printed to
/// stderr while it initialises Vulkan) never break the protocol's one-JSON-object-per-line rule.
/// Falls back to plain stderr if anything fails.
pub fn protocol_stream() -> Box<dyn Write + Send> {
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        // SAFETY: dup/open/dup2/close on file descriptors we own; the duplicate is handed to a
        // File that owns it from then on.
        unsafe {
            let copy = libc::dup(2);
            if copy < 0 {
                return Box::new(std::io::stderr());
            }
            let null = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
            if null >= 0 {
                libc::dup2(null, 2);
                libc::close(null);
            }
            libc::fcntl(copy, libc::F_SETFD, libc::FD_CLOEXEC);
            Box::new(std::fs::File::from_raw_fd(copy))
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::FromRawHandle;
        // SAFETY: CRT descriptor calls on descriptors we own; the duplicated OS handle is owned
        // by the returned File (the CRT descriptor `copy` is leaked on purpose with it).
        unsafe {
            let copy = libc::dup(2);
            if copy < 0 {
                return Box::new(std::io::stderr());
            }
            let null = libc::open(c"NUL".as_ptr(), libc::O_WRONLY);
            if null >= 0 {
                libc::dup2(null, 2);
                libc::close(null);
            }
            let handle = libc::get_osfhandle(copy);
            if handle == -1 {
                return Box::new(std::io::stderr());
            }
            Box::new(std::fs::File::from_raw_handle(handle as _))
        }
    }
}
