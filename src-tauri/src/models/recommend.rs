//! Recommendation engine: pure function from a hardware report to a ranked,
//! fit-annotated set of model picks. No I/O, fully unit-tested.

use serde::{Deserialize, Serialize};

use super::{catalog, CatalogEntry, Role};
use crate::error::Result;
use crate::hardware::{ComputeClass, HardwareReport};

/// Fraction of VRAM treated as usable — the OS/compositor and driver hold the rest.
const VRAM_USABLE_FRACTION: f64 = 0.95;
/// Flat runtime reserve (GiB) for the inference server itself.
const RUNTIME_RESERVE_GB: f64 = 0.5;
/// CPU-only machines: keep half of system RAM for the OS and the user's apps.
const CPU_RAM_FRACTION: f64 = 0.50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceMode {
    GpuFull,
    CpuOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    pub entry_id: String,
    pub name: String,
    pub family: String,
    pub params_b: f64,
    pub roles: Vec<Role>,
    pub quality: u32,
    pub blurb: String,
    pub quant: String,
    pub file_gb: f64,
    pub est_mem_gb: f64,
    pub headroom_gb: f64,
    pub headroom_pct: f64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePick {
    pub role: Role,
    pub pick: Pick,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationSet {
    pub mode: InferenceMode,
    pub compute_class: ComputeClass,
    pub budget_gb: f64,
    pub best: Option<Pick>,
    pub alternates: Vec<Pick>,
    pub by_role: Vec<RolePick>,
    pub notes: Vec<String>,
}

fn budget_for(report: &HardwareReport) -> (InferenceMode, f64) {
    let vram = report.max_gpu_vram_gb();
    if vram >= 2.0 {
        (
            InferenceMode::GpuFull,
            (vram * VRAM_USABLE_FRACTION - RUNTIME_RESERVE_GB).max(0.0),
        )
    } else {
        (InferenceMode::CpuOnly, report.ram_gb() * CPU_RAM_FRACTION)
    }
}

/// Best quant for an entry within a budget: largest memory floor that fits,
/// i.e. the highest-quality quantization the machine can actually hold.
fn best_fitting_quant(entry: &CatalogEntry, budget_gb: f64) -> Option<&super::QuantOption> {
    entry
        .quants
        .iter()
        .filter(|q| q.min_mem_gb <= budget_gb)
        .max_by(|a, b| a.min_mem_gb.total_cmp(&b.min_mem_gb))
}

fn fit_note(headroom_gb: f64, mode: InferenceMode) -> String {
    match mode {
        InferenceMode::CpuOnly => {
            "Runs on CPU — expect a few tokens/sec. Fine for drafts, not conversation.".into()
        }
        InferenceMode::GpuFull => {
            if headroom_gb >= 8.0 {
                format!(
                    "Fits with {headroom_gb:.0} GB to spare — room for an embedding model and long context alongside."
                )
            } else if headroom_gb >= 2.0 {
                format!("Comfortable fit with {headroom_gb:.1} GB of headroom at 8K context.")
            } else {
                "Tight fit — keep context at 8K and close other GPU-heavy apps.".into()
            }
        }
    }
}

fn make_pick(entry: &CatalogEntry, budget_gb: f64, mode: InferenceMode) -> Option<Pick> {
    let quant = best_fitting_quant(entry, budget_gb)?;
    let headroom_gb = budget_gb - quant.min_mem_gb;
    Some(Pick {
        entry_id: entry.id.clone(),
        name: entry.name.clone(),
        family: entry.family.clone(),
        params_b: entry.params_b,
        roles: entry.roles.clone(),
        quality: entry.quality,
        blurb: entry.blurb.clone(),
        quant: quant.label.clone(),
        file_gb: quant.file_gb,
        est_mem_gb: quant.min_mem_gb,
        headroom_gb,
        headroom_pct: if budget_gb > 0.0 {
            (headroom_gb / budget_gb * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        },
        note: fit_note(headroom_gb, mode),
    })
}

pub fn recommend(report: &HardwareReport) -> Result<RecommendationSet> {
    let cat = catalog()?;
    let (mode, budget_gb) = budget_for(report);

    // Chat-capable entries, best first. Embedding models are recommended per-role
    // only — they are companions, never the headline pick.
    let mut fitting: Vec<Pick> = cat
        .entries
        .iter()
        .filter(|e| !e.roles.contains(&Role::Embedding))
        .filter_map(|e| make_pick(e, budget_gb, mode))
        .collect();
    fitting.sort_by(|a, b| b.quality.cmp(&a.quality));

    let best = fitting.first().cloned();
    let alternates: Vec<Pick> = fitting.iter().skip(1).take(3).cloned().collect();

    let mut by_role = Vec::new();
    for role in [Role::General, Role::Coding, Role::Reasoning, Role::Embedding] {
        let role_best = cat
            .entries
            .iter()
            .filter(|e| e.roles.contains(&role))
            .filter_map(|e| make_pick(e, budget_gb, mode))
            .max_by_key(|p| p.quality);
        if let Some(pick) = role_best {
            by_role.push(RolePick { role, pick });
        }
    }

    let mut notes = Vec::new();
    match mode {
        InferenceMode::CpuOnly => notes.push(
            "No dedicated GPU detected — recommendations target CPU inference in system RAM."
                .to_string(),
        ),
        InferenceMode::GpuFull => notes.push(format!(
            "Budget: {budget_gb:.1} GB usable of {:.0} GB VRAM ({:.0}% usable minus {RUNTIME_RESERVE_GB:.1} GB runtime reserve).",
            report.max_gpu_vram_gb(),
            VRAM_USABLE_FRACTION * 100.0
        )),
    }
    if best.is_none() {
        notes.push(
            "This machine is below the floor for local chat models. Cloud-assisted mode is the honest recommendation.".to_string(),
        );
    }

    Ok(RecommendationSet {
        mode,
        compute_class: report.compute_class,
        budget_gb,
        best,
        alternates,
        by_role,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{classify, CpuInfo, GpuInfo, GpuVendor, MemoryInfo, OsInfo};

    fn report_with(vram_gb: Option<f64>, ram_gb: f64) -> HardwareReport {
        const GIB: u64 = 1024 * 1024 * 1024;
        let gpus = vram_gb
            .map(|v| {
                vec![GpuInfo {
                    name: "Test GPU".into(),
                    vendor: GpuVendor::Nvidia,
                    vram_total_bytes: Some((v * GIB as f64) as u64),
                    vram_used_bytes: None,
                    driver_version: None,
                    cuda_version: None,
                    temperature_c: None,
                    utilization_pct: None,
                    source: "test".into(),
                }]
            })
            .unwrap_or_default();
        HardwareReport {
            cpu: CpuInfo {
                brand: "Test CPU".into(),
                physical_cores: Some(8),
                logical_cores: 16,
                base_frequency_mhz: 3600,
                arch: "x86_64".into(),
            },
            memory: MemoryInfo {
                total_bytes: (ram_gb * GIB as f64) as u64,
                available_bytes: (ram_gb * GIB as f64 * 0.7) as u64,
            },
            gpus,
            disks: vec![],
            os: OsInfo {
                name: "Windows".into(),
                version: "11".into(),
                hostname: "test".into(),
                arch: "x86_64".into(),
            },
            compute_class: classify(vram_gb.unwrap_or(0.0)),
            detected_at: "2026-07-03T00:00:00Z".into(),
        }
    }

    #[test]
    fn workstation_gets_the_flagship() {
        let recs = recommend(&report_with(Some(96.0), 256.0)).unwrap();
        assert_eq!(recs.mode, InferenceMode::GpuFull);
        let best = recs.best.expect("96GB must have a best pick");
        assert_eq!(best.entry_id, "llama-3.3-70b-instruct");
        assert!(best.headroom_gb > 30.0);
    }

    #[test]
    fn mid_range_24gb_lands_on_32b_class() {
        // 24 * 0.95 - 0.5 = 22.3 budget: Qwen3 32B Q4_K_M (22.0) fits — the
        // classic 3090/4090 experience — but nothing from the 70B tier leaks in.
        let recs = recommend(&report_with(Some(24.0), 64.0)).unwrap();
        let best = recs.best.unwrap();
        assert_eq!(best.entry_id, "qwen3-32b");
        assert!(best.est_mem_gb <= recs.budget_gb);
    }

    #[test]
    fn eight_gb_card_gets_a_small_model() {
        let recs = recommend(&report_with(Some(8.0), 32.0)).unwrap();
        let best = recs.best.unwrap();
        assert!(best.est_mem_gb <= 8.0 * 0.95 - 0.5);
        assert!(best.params_b <= 9.0, "8GB card must get <=9B params, got {}", best.params_b);
    }

    #[test]
    fn cpu_only_still_recommends() {
        let recs = recommend(&report_with(None, 16.0)).unwrap();
        assert_eq!(recs.mode, InferenceMode::CpuOnly);
        let best = recs.best.expect("16GB RAM machine can run something");
        assert!(best.est_mem_gb <= 8.0);
    }

    #[test]
    fn tiny_machine_degrades_honestly() {
        // 4 GB RAM -> 2 GB budget: nothing in the catalog fits, and the notes say so.
        let recs = recommend(&report_with(None, 4.0)).unwrap();
        assert!(recs.best.is_none());
        assert!(recs.notes.iter().any(|n| n.contains("below the floor")));
    }

    #[test]
    fn role_picks_cover_all_roles_on_big_hardware() {
        let recs = recommend(&report_with(Some(48.0), 128.0)).unwrap();
        assert_eq!(recs.by_role.len(), 4);
        let coding = recs
            .by_role
            .iter()
            .find(|r| r.role == Role::Coding)
            .unwrap();
        assert!(coding.pick.roles.contains(&Role::Coding));
    }

    #[test]
    fn sixteen_gb_card_gets_gpt_oss() {
        // 16 * 0.95 - 0.5 = 14.7 budget: GPT-OSS 20B (13.5) is the quality leader that fits.
        let recs = recommend(&report_with(Some(16.0), 64.0)).unwrap();
        let best = recs.best.unwrap();
        assert_eq!(best.entry_id, "gpt-oss-20b");
        assert!(best.est_mem_gb <= recs.budget_gb);
    }
}
