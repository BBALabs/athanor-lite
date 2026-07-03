//! Hardware intelligence: one-shot detection + the compute classification that
//! drives every model recommendation and runtime decision in the app.

pub mod gpu;
pub mod telemetry;

use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};

use crate::error::Result;

pub const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub brand: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub base_frequency_mhz: u64,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub vendor: GpuVendor,
    pub vram_total_bytes: Option<u64>,
    pub vram_used_bytes: Option<u64>,
    pub driver_version: Option<String>,
    pub cuda_version: Option<String>,
    pub temperature_c: Option<u32>,
    pub utilization_pct: Option<u32>,
    /// Which probe produced this record: "nvml" (live-capable) or "wmi" (static).
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub hostname: String,
    pub arch: String,
}

/// Distilled capability tier — the single input the recommender and (later) the
/// runtime configurator key off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputeClass {
    CpuOnly,
    VramLow,
    VramMid,
    VramHigh,
    VramWorkstation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareReport {
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub gpus: Vec<GpuInfo>,
    pub disks: Vec<DiskInfo>,
    pub os: OsInfo,
    pub compute_class: ComputeClass,
    pub detected_at: String,
}

impl HardwareReport {
    /// VRAM of the largest single GPU, in GiB. Multi-GPU splits are an M2+
    /// concern; recommendations stay honest by assuming one device.
    pub fn max_gpu_vram_gb(&self) -> f64 {
        self.gpus
            .iter()
            .filter_map(|g| g.vram_total_bytes)
            .max()
            .map(|b| b as f64 / GIB)
            .unwrap_or(0.0)
    }

    pub fn ram_gb(&self) -> f64 {
        self.memory.total_bytes as f64 / GIB
    }
}

pub fn classify(max_vram_gb: f64) -> ComputeClass {
    if max_vram_gb < 2.0 {
        ComputeClass::CpuOnly
    } else if max_vram_gb < 6.0 {
        ComputeClass::VramLow
    } else if max_vram_gb < 16.0 {
        ComputeClass::VramMid
    } else if max_vram_gb < 32.0 {
        ComputeClass::VramHigh
    } else {
        ComputeClass::VramWorkstation
    }
}

pub fn detect() -> Result<HardwareReport> {
    let started = std::time::Instant::now();

    let mut sys = System::new_all();
    sys.refresh_all();

    let cpus = sys.cpus();
    let cpu = CpuInfo {
        brand: cpus
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_else(|| "Unknown CPU".into()),
        physical_cores: sys.physical_core_count(),
        logical_cores: cpus.len(),
        base_frequency_mhz: cpus.first().map(|c| c.frequency()).unwrap_or(0),
        arch: std::env::consts::ARCH.to_string(),
    };

    let memory = MemoryInfo {
        total_bytes: sys.total_memory(),
        available_bytes: sys.available_memory(),
    };

    let gpus = gpu::detect_gpus();

    let disks = Disks::new_with_refreshed_list()
        .iter()
        .map(|d| DiskInfo {
            name: d.name().to_string_lossy().to_string(),
            mount: d.mount_point().to_string_lossy().to_string(),
            total_bytes: d.total_space(),
            available_bytes: d.available_space(),
            kind: format!("{:?}", d.kind()),
        })
        .collect();

    let os = OsInfo {
        name: System::name().unwrap_or_else(|| "Unknown OS".into()),
        version: System::os_version().unwrap_or_default(),
        hostname: System::host_name().unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
    };

    let max_vram_gb = gpus
        .iter()
        .filter_map(|g: &GpuInfo| g.vram_total_bytes)
        .max()
        .map(|b| b as f64 / GIB)
        .unwrap_or(0.0);

    let report = HardwareReport {
        cpu,
        memory,
        gpus,
        disks,
        os,
        compute_class: classify(max_vram_gb),
        detected_at: chrono::Utc::now().to_rfc3339(),
    };

    log::info!(
        target: "hw",
        "detection complete in {:?}: {} | {:.0} GiB RAM | {} GPU(s) | class {:?}",
        started.elapsed(),
        report.cpu.brand,
        report.ram_gb(),
        report.gpus.len(),
        report.compute_class
    );

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_boundaries() {
        assert_eq!(classify(0.0), ComputeClass::CpuOnly);
        assert_eq!(classify(4.0), ComputeClass::VramLow);
        assert_eq!(classify(8.0), ComputeClass::VramMid);
        assert_eq!(classify(24.0), ComputeClass::VramHigh);
        assert_eq!(classify(48.0), ComputeClass::VramWorkstation);
    }
}
