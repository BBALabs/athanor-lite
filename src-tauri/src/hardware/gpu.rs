//! GPU probing.
//!
//! Strategy (in order of trust):
//! 1. NVML — exact VRAM totals/usage, driver + CUDA versions, live telemetry (NVIDIA only).
//! 2. WMI `Win32_VideoController` — enumerates every adapter (any vendor), but its
//!    `AdapterRAM` field is a u32 and silently caps at 4 GB. Never trusted for size.
//! 3. Registry `HardwareInformation.qwMemorySize` — the accurate 64-bit VRAM total,
//!    matched to WMI adapters by driver description.
//!
//! Newer architectures: the GPU generation is derived from CUDA compute capability
//! (a stable driver-reported number) rather than a name table, so Blackwell-class
//! and future cards identify correctly without a wrapper/enum update. If a driver
//! predates its GPU and NVML reports a generic name ("NVIDIA Graphics Device"),
//! the registry's driver description is substituted when it is unambiguous.

use std::sync::OnceLock;

use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use nvml_wrapper::Nvml;

use super::{GpuInfo, GpuVendor};

/// NVML handle, initialized once per process. `None` on machines without an
/// NVIDIA driver — every caller degrades gracefully.
pub fn nvml() -> Option<&'static Nvml> {
    static NVML: OnceLock<Option<Nvml>> = OnceLock::new();
    NVML.get_or_init(|| match Nvml::init() {
        Ok(n) => Some(n),
        Err(e) => {
            log::info!(target: "hw", "NVML unavailable ({e}); falling back to WMI probe");
            None
        }
    })
    .as_ref()
}

fn classify_vendor(name: &str) -> GpuVendor {
    let n = name.to_ascii_lowercase();
    if ["nvidia", "geforce", "rtx", "gtx", "quadro", "tesla"]
        .iter()
        .any(|k| n.contains(k))
    {
        GpuVendor::Nvidia
    } else if n.contains("amd") || n.contains("radeon") || n.contains("firepro") {
        GpuVendor::Amd
    } else if n.contains("intel") || n.contains("arc") || n.contains("iris") || n.contains("uhd") {
        GpuVendor::Intel
    } else {
        GpuVendor::Other
    }
}

/// CUDA compute capability → architecture name. Compute capability is reported
/// by the driver itself, so this stays correct for cards newer than any name
/// table — only genuinely new generations need a row here.
fn architecture_for(major: i32, minor: i32) -> Option<&'static str> {
    Some(match (major, minor) {
        (12, _) | (10, _) => "Blackwell",
        (9, _) => "Hopper",
        (8, 9) => "Ada Lovelace",
        (8, _) => "Ampere",
        (7, 5) => "Turing",
        (7, _) => "Volta",
        (6, _) => "Pascal",
        (5, _) => "Maxwell",
        (3, _) => "Kepler",
        _ => return None,
    })
}

/// NVML reports a placeholder when the installed driver predates the GPU.
fn is_generic_nvml_name(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    n == "nvidia graphics device" || n == "graphics device" || n == "nvidia gpu"
}

fn detect_nvml_gpus() -> Vec<GpuInfo> {
    let Some(nvml) = nvml() else {
        return Vec::new();
    };

    let driver = nvml.sys_driver_version().ok();
    let cuda = nvml.sys_cuda_driver_version().ok().map(|v| {
        format!(
            "{}.{}",
            nvml_wrapper::cuda_driver_version_major(v),
            nvml_wrapper::cuda_driver_version_minor(v)
        )
    });

    let count = nvml.device_count().unwrap_or(0);
    let mut gpus = Vec::with_capacity(count as usize);

    for i in 0..count {
        let Ok(dev) = nvml.device_by_index(i) else {
            continue;
        };
        let mem = dev.memory_info().ok();
        let util = dev.utilization_rates().ok();
        let cc = dev.cuda_compute_capability().ok();

        let mut name = dev.name().unwrap_or_else(|_| "NVIDIA GPU".into());
        if is_generic_nvml_name(&name) {
            #[cfg(windows)]
            if let Some(desc) = windows_probe::sole_nvidia_driver_desc() {
                log::info!(
                    target: "hw",
                    "NVML reported generic name {name:?}; using registry driver description {desc:?}"
                );
                name = desc;
            }
        }

        gpus.push(GpuInfo {
            name,
            vendor: GpuVendor::Nvidia,
            vram_total_bytes: mem.as_ref().map(|m| m.total),
            vram_used_bytes: mem.as_ref().map(|m| m.used),
            driver_version: driver.clone(),
            cuda_version: cuda.clone(),
            architecture: cc
                .as_ref()
                .and_then(|c| architecture_for(c.major, c.minor))
                .map(str::to_string),
            compute_capability: cc.as_ref().map(|c| format!("{}.{}", c.major, c.minor)),
            temperature_c: dev.temperature(TemperatureSensor::Gpu).ok(),
            utilization_pct: util.map(|u| u.gpu),
            source: "nvml".into(),
        });
    }
    gpus
}

/// Full adapter sweep. NVML devices first (authoritative), then any adapter WMI
/// knows about that NVML didn't cover, with VRAM totals pulled from the registry.
pub fn detect_gpus() -> Vec<GpuInfo> {
    let mut gpus = detect_nvml_gpus();

    #[cfg(windows)]
    {
        let wmi_gpus = windows_probe::detect_wmi_gpus();
        // NVML fully accounts for WMI's NVIDIA adapters only when it enumerated
        // at least as many — otherwise (a card disabled, errored, or on a legacy
        // driver) the WMI record is the only trace of it and must survive.
        let nvml_nvidia = gpus.iter().filter(|g| g.vendor == GpuVendor::Nvidia).count();
        let wmi_nvidia = wmi_gpus.iter().filter(|g| g.vendor == GpuVendor::Nvidia).count();
        let nvml_covers_all_nvidia = nvml_nvidia >= wmi_nvidia;

        for wmi_gpu in wmi_gpus {
            let already_covered = gpus.iter().any(|g| {
                g.name.eq_ignore_ascii_case(&wmi_gpu.name)
                    || g.name.contains(&wmi_gpu.name)
                    || wmi_gpu.name.contains(&g.name)
                    || (nvml_covers_all_nvidia
                        && g.source == "nvml"
                        && wmi_gpu.vendor == GpuVendor::Nvidia)
            });
            if !already_covered {
                gpus.push(wmi_gpu);
            }
        }
    }

    // Largest VRAM first — the primary inference device leads everywhere in the UI.
    gpus.sort_by_key(|g| std::cmp::Reverse(g.vram_total_bytes.unwrap_or(0)));

    for g in &gpus {
        let vram = g
            .vram_total_bytes
            .map(|b| format!("{:.1} GiB", b as f64 / super::GIB))
            .unwrap_or_else(|| "unknown".into());
        log::info!(
            target: "hw",
            "gpu: {:?} | {:?} | vram {} | arch {} | cc {} | driver {} | source {}",
            g.name,
            g.vendor,
            vram,
            g.architecture.as_deref().unwrap_or("unknown"),
            g.compute_capability.as_deref().unwrap_or("-"),
            g.driver_version.as_deref().unwrap_or("-"),
            g.source
        );
    }

    gpus
}

#[cfg(windows)]
mod windows_probe {
    use std::collections::HashMap;

    use serde::Deserialize;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::{RegKey, RegValue};

    use super::{classify_vendor, GpuInfo};

    #[derive(Deserialize, Debug)]
    struct Win32VideoController {
        #[serde(rename = "Name")]
        name: Option<String>,
        #[serde(rename = "DriverVersion")]
        driver_version: Option<String>,
    }

    /// Display-class GUID under which Windows records the true 64-bit VRAM size.
    const DISPLAY_CLASS_KEY: &str =
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

    /// Read a registry memory-size value regardless of how the driver stored it:
    /// REG_QWORD, REG_DWORD, or a raw little-endian REG_BINARY blob.
    fn read_memory_size(key: &RegKey, value_name: &str) -> Option<u64> {
        if let Ok(v) = key.get_value::<u64, _>(value_name) {
            return Some(v);
        }
        if let Ok(v) = key.get_value::<u32, _>(value_name) {
            return Some(v as u64);
        }
        let raw: RegValue = key.get_raw_value(value_name).ok()?;
        // Only decode raw blobs; an 8-byte REG_SZ must not become a VRAM total.
        if raw.vtype != winreg::enums::RegType::REG_BINARY {
            return None;
        }
        match raw.bytes.len() {
            8 => Some(u64::from_le_bytes(raw.bytes[..8].try_into().ok()?)),
            4 => Some(u32::from_le_bytes(raw.bytes[..4].try_into().ok()?) as u64),
            _ => None,
        }
    }

    fn display_class_entries() -> Vec<(String, Option<u64>)> {
        let mut out = Vec::new();
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(class) = hklm.open_subkey(DISPLAY_CLASS_KEY) else {
            return out;
        };
        for sub in class.enum_keys().flatten() {
            if !sub.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let Ok(key) = class.open_subkey(&sub) else {
                continue;
            };
            let Ok(desc) = key.get_value::<String, _>("DriverDesc") else {
                continue;
            };
            // qwMemorySize is authoritative; MemorySize is the legacy fallback.
            let size = read_memory_size(&key, "HardwareInformation.qwMemorySize")
                .or_else(|| read_memory_size(&key, "HardwareInformation.MemorySize"))
                .filter(|&s| s > 0);
            out.push((desc, size));
        }
        out
    }

    /// DriverDesc -> VRAM bytes, for matching accurate totals to WMI names.
    fn registry_vram_by_desc() -> HashMap<String, u64> {
        display_class_entries()
            .into_iter()
            .filter_map(|(desc, size)| size.map(|s| (desc, s)))
            .collect()
    }

    /// The registry driver description of the NVIDIA adapter, if exactly one
    /// exists — used to replace NVML's generic placeholder name. Ambiguous
    /// (multi-NVIDIA) systems return None rather than guess.
    pub(super) fn sole_nvidia_driver_desc() -> Option<String> {
        let mut nvidia: Vec<String> = display_class_entries()
            .into_iter()
            .map(|(desc, _)| desc)
            .filter(|d| d.to_ascii_lowercase().contains("nvidia"))
            .collect();
        nvidia.dedup();
        if nvidia.len() == 1 {
            nvidia.pop()
        } else {
            None
        }
    }

    pub(super) fn detect_wmi_gpus() -> Vec<GpuInfo> {
        let controllers: Vec<Win32VideoController> = (|| {
            let com = wmi::COMLibrary::new()?;
            let conn = wmi::WMIConnection::new(com)?;
            conn.raw_query("SELECT Name, DriverVersion FROM Win32_VideoController")
        })()
        .unwrap_or_else(|e: wmi::WMIError| {
            log::warn!(target: "hw", "WMI video controller query failed: {e}");
            Vec::new()
        });

        let vram_map = registry_vram_by_desc();

        controllers
            .into_iter()
            .filter_map(|c| {
                let name = c.name?;
                // Virtual/remote display adapters are noise, not compute devices.
                let lower = name.to_ascii_lowercase();
                if lower.contains("microsoft") || lower.contains("virtual") {
                    return None;
                }
                let vram = vram_map.get(&name).copied();
                Some(GpuInfo {
                    vendor: classify_vendor(&name),
                    vram_total_bytes: vram,
                    vram_used_bytes: None,
                    driver_version: c.driver_version,
                    cuda_version: None,
                    architecture: None,
                    compute_capability: None,
                    temperature_c: None,
                    utilization_pct: None,
                    source: "wmi".into(),
                    name,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn architecture_mapping_covers_current_generations() {
        assert_eq!(architecture_for(12, 0), Some("Blackwell")); // RTX PRO 6000 Blackwell
        assert_eq!(architecture_for(10, 0), Some("Blackwell")); // B100/B200
        assert_eq!(architecture_for(9, 0), Some("Hopper"));
        assert_eq!(architecture_for(8, 9), Some("Ada Lovelace"));
        assert_eq!(architecture_for(8, 6), Some("Ampere"));
        assert_eq!(architecture_for(7, 5), Some("Turing"));
        assert_eq!(architecture_for(6, 1), Some("Pascal"));
        assert_eq!(architecture_for(99, 0), None); // future: unknown, never wrong
    }

    #[test]
    fn generic_names_are_recognized() {
        assert!(is_generic_nvml_name("NVIDIA Graphics Device"));
        assert!(is_generic_nvml_name("Graphics Device"));
        assert!(!is_generic_nvml_name(
            "NVIDIA RTX PRO 6000 Blackwell Workstation Edition"
        ));
    }

    #[test]
    fn vendor_classification() {
        assert_eq!(
            classify_vendor("NVIDIA RTX PRO 6000 Blackwell Workstation Edition"),
            GpuVendor::Nvidia
        );
        assert_eq!(classify_vendor("AMD Radeon RX 7900 XTX"), GpuVendor::Amd);
        assert_eq!(classify_vendor("Intel Arc A770"), GpuVendor::Intel);
    }
}
