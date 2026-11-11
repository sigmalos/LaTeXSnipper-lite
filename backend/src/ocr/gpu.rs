//! GPU acceleration for ONNX Runtime sessions.
//!
//! * Windows: DirectML on the hardware adapter with the most dedicated video
//!   memory (NVIDIA, AMD and Intel all work through Direct3D 12).
//! * Linux: CUDA, when `libonnxruntime_providers_cuda.so` and the CUDA 12 /
//!   cuDNN 9 runtime are installed (the provider is ~400 MB, so it is not
//!   bundled; see [`cuda_library_dirs`]).
//! * macOS and anything else: CPU only.
//!
//! Every GPU step is best-effort. If no GPU is found, the provider cannot be
//! registered, a model fails to load on it or a run fails, the caller falls
//! back to the CPU. Set `LATEXSNIPPER_DEVICE=cpu` to skip the GPU entirely.

use ort::session::builder::SessionBuilder;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// A GPU execution provider that looked usable when the app probed for it.
#[derive(Clone, Debug)]
pub struct Gpu {
    /// Provider name for logs, e.g. "DirectML".
    pub provider: &'static str,
    /// Adapter description for logs.
    pub name: String,
    #[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
    device_id: i32,
}

static PROBE: OnceLock<Option<Gpu>> = OnceLock::new();
/// Set once the provider itself turned out to be unusable, so later sessions
/// go straight to the CPU instead of failing the same way again.
static BROKEN: AtomicBool = AtomicBool::new(false);

/// The GPU to try for new sessions, or `None` to use the CPU.
pub fn preferred() -> Option<&'static Gpu> {
    // macOS keeps its CPU-only behaviour unchanged: no probing, no logging.
    if cfg!(not(any(windows, target_os = "linux"))) || BROKEN.load(Ordering::Relaxed) {
        return None;
    }
    PROBE
        .get_or_init(|| {
            if cpu_forced(std::env::var("LATEXSNIPPER_DEVICE").ok().as_deref()) {
                eprintln!("ocr: LATEXSNIPPER_DEVICE=cpu, GPU disabled");
                return None;
            }
            match probe() {
                Ok(gpu) => {
                    eprintln!("ocr: found GPU {} ({})", gpu.name, gpu.provider);
                    Some(gpu)
                }
                Err(why) => {
                    eprintln!("ocr: no usable GPU, using CPU: {why}");
                    None
                }
            }
        })
        .as_ref()
}

/// Stop offering the GPU for the rest of this run.
pub fn disable(why: &str) {
    if !BROKEN.swap(true, Ordering::Relaxed) {
        eprintln!("ocr: GPU disabled, using CPU: {why}");
    }
}

fn cpu_forced(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.trim().eq_ignore_ascii_case("cpu"))
}

/// Add the GPU execution provider to `builder`, failing (rather than silently
/// running on the CPU) if ONNX Runtime cannot create it.
pub fn register(builder: SessionBuilder, gpu: &Gpu) -> ort::Result<SessionBuilder> {
    #[cfg(windows)]
    {
        use ort::execution_providers::DirectMLExecutionProvider;
        // DirectML cannot use memory patterns or run nodes in parallel.
        builder
            .with_memory_pattern(false)?
            .with_parallel_execution(false)?
            .with_execution_providers([DirectMLExecutionProvider::default()
                .with_device_id(gpu.device_id)
                .build()
                .error_on_failure()])
    }
    #[cfg(target_os = "linux")]
    {
        use ort::execution_providers::cuda::CuDNNConvAlgorithmSearch;
        use ort::execution_providers::CUDAExecutionProvider;
        // Input sizes vary from image to image; an exhaustive cuDNN search
        // for every new shape would cost more than it saves.
        builder.with_execution_providers([CUDAExecutionProvider::default()
            .with_device_id(gpu.device_id)
            .with_conv_algorithm_search(CuDNNConvAlgorithmSearch::Heuristic)
            .build()
            .error_on_failure()])
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = gpu;
        Ok(builder)
    }
}

// ------------------------------------------------------------------ Windows

#[cfg(windows)]
fn probe() -> Result<Gpu, String> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    };

    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|e| format!("DXGI unavailable: {e}"))?;
    let mut best: Option<(u32, usize, String)> = None;
    // DirectML's `device_id` is this same EnumAdapters1 index.
    for index in 0u32.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
            break;
        };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        // Skip WARP / Microsoft Basic Render Driver; ONNX Runtime refuses it anyway.
        let software = desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0
            || (desc.VendorId == 0x1414 && desc.DeviceId == 0x8c);
        if software {
            continue;
        }
        let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(128);
        let name = String::from_utf16_lossy(&desc.Description[..len]);
        // Prefer the discrete card: the one with the most dedicated memory.
        if best
            .as_ref()
            .is_none_or(|b| desc.DedicatedVideoMemory > b.1)
        {
            best = Some((index, desc.DedicatedVideoMemory, name));
        }
    }
    let (index, _, name) = best.ok_or("no Direct3D 12 hardware adapter")?;
    Ok(Gpu {
        provider: "DirectML",
        name,
        device_id: index as i32,
    })
}

// -------------------------------------------------------------------- Linux

/// Where the CUDA provider libraries are looked for, in order. Anything not
/// found here is left to the dynamic linker (`LD_LIBRARY_PATH`, ldconfig).
#[cfg(target_os = "linux")]
pub fn cuda_library_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("LATEXSNIPPER_CUDA_DIR") {
        dirs.push(dir.into());
    }
    dirs.push(crate::data_dir().join("onnxruntime-cuda"));
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
    {
        dirs.push(dir);
    }
    dirs
}

#[cfg(target_os = "linux")]
fn probe() -> Result<Gpu, String> {
    const SHARED: &str = "libonnxruntime_providers_shared.so";
    const CUDA: &str = "libonnxruntime_providers_cuda.so";

    // Without an NVIDIA driver there is nothing to try. WSL 2 exposes the
    // Windows driver through /dev/dxg instead of /proc/driver/nvidia.
    let driver = match std::fs::read_to_string("/proc/driver/nvidia/version") {
        Ok(v) => v,
        Err(_) if std::path::Path::new("/dev/dxg").exists() => "NVIDIA GPU (WSL)".into(),
        Err(_) => return Err("no NVIDIA driver loaded".into()),
    };
    let name = std::fs::read_dir("/proc/driver/nvidia/gpus")
        .ok()
        .and_then(|mut gpus| gpus.next())
        .and_then(|gpu| std::fs::read_to_string(gpu.ok()?.path().join("information")).ok())
        .and_then(|info| {
            info.lines()
                .find_map(|l| l.strip_prefix("Model:").map(|m| m.trim().to_string()))
        })
        .unwrap_or_else(|| {
            driver
                .lines()
                .next()
                .unwrap_or("NVIDIA GPU")
                .trim()
                .to_string()
        });

    // ONNX Runtime dlopen()s the providers by bare file name. Loading them
    // first from a known directory makes that lookup find the loaded copy
    // (the libraries carry a matching SONAME). The shared bridge needs
    // RTLD_GLOBAL, exactly as ONNX Runtime itself would open it.
    let dir = cuda_library_dirs()
        .into_iter()
        .find(|d| d.join(CUDA).is_file() && d.join(SHARED).is_file());
    match &dir {
        Some(dir) => {
            dlopen(&dir.join(SHARED), libc::RTLD_NOW | libc::RTLD_GLOBAL)?;
            dlopen(&dir.join(CUDA), libc::RTLD_NOW | libc::RTLD_LOCAL)?;
        }
        None => {
            // Maybe installed system-wide; check before ONNX Runtime tries.
            let hint = || {
                format!(
                    "{CUDA} not found; copy it and {SHARED} from the ONNX Runtime 1.22 \
                     GPU build into {}",
                    crate::data_dir().join("onnxruntime-cuda").display()
                )
            };
            dlopen_name(SHARED, libc::RTLD_NOW | libc::RTLD_GLOBAL).map_err(|_| hint())?;
            dlopen_name(CUDA, libc::RTLD_NOW | libc::RTLD_LOCAL)?;
        }
    }
    Ok(Gpu {
        provider: "CUDA",
        name,
        device_id: 0,
    })
}

#[cfg(target_os = "linux")]
fn dlopen(path: &std::path::Path, flags: libc::c_int) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    dlopen_c(&c, flags)
}

#[cfg(target_os = "linux")]
fn dlopen_name(name: &str, flags: libc::c_int) -> Result<(), String> {
    let c = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
    dlopen_c(&c, flags)
}

/// Load a library for the rest of the process lifetime (never closed).
#[cfg(target_os = "linux")]
fn dlopen_c(name: &std::ffi::CStr, flags: libc::c_int) -> Result<(), String> {
    // SAFETY: plain C calls with a valid NUL-terminated string; dlerror()'s
    // result is copied out immediately.
    unsafe {
        if libc::dlopen(name.as_ptr(), flags).is_null() {
            let err = libc::dlerror();
            let msg = if err.is_null() {
                "unknown error".to_string()
            } else {
                std::ffi::CStr::from_ptr(err).to_string_lossy().into_owned()
            };
            return Err(format!("cannot load {}: {msg}", name.to_string_lossy()));
        }
    }
    Ok(())
}

// -------------------------------------------------------------------- other

#[cfg(not(any(windows, target_os = "linux")))]
fn probe() -> Result<Gpu, String> {
    Err("GPU acceleration is only used on Windows and Linux".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_override() {
        assert!(cpu_forced(Some("cpu")));
        assert!(cpu_forced(Some(" CPU ")));
        assert!(!cpu_forced(Some("gpu")));
        assert!(!cpu_forced(Some("")));
        assert!(!cpu_forced(None));
    }
}
