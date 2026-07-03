//! Recommendation engine: a pure function from a hardware report to a ranked,
//! fit-annotated set of model picks — plus a fit verdict for every catalog
//! quant so the UI shows the *same* numbers everywhere. No I/O, fully tested.
//!
//! The memory model decomposes each quant's catalog `minMemGb` (measured at
//! 8K context) into weights + KV-cache + fixed overhead, which lets fit be
//! computed at any context length without new per-model data:
//!
//!   weights_gb    = file size on disk
//!   overhead_gb   = OVERHEAD_GB (CUDA context + compute buffers)
//!   kv_per_token  = (minMemGb − weights − overhead) / 8192
//!   mem(ctx)      = weights + overhead + kv_per_token · ctx
//!
//! Budget is honest about reality: it subtracts VRAM already in use, sums
//! eligible NVIDIA GPUs (tensor-split), and models CPU offload for the
//! near-fits. "Tight" only ever means "fits with little headroom" — a model
//! over budget is a partial-offload candidate or simply doesn't fit, never
//! "tight".

use serde::{Deserialize, Serialize};

use super::{catalog, CatalogEntry, QuantOption, Role};
use crate::error::Result;
use crate::hardware::{ComputeClass, GpuVendor, HardwareReport, GIB};

/// Fraction of a GPU's *total* VRAM usable before the driver/compositor floor.
const VRAM_USABLE_FRACTION: f64 = 0.95;
/// Fixed per-model runtime cost (GiB): CUDA context + compute buffers.
const OVERHEAD_GB: f64 = 0.5;
/// Context at which catalog `minMemGb` values were measured.
const KV_REF_CTX: f64 = 8192.0;
/// Context the recommender evaluates fit at (matches the runtime default).
pub const DEFAULT_CTX: u32 = 8192;
/// CPU-only budget: keep half of system RAM for the OS and the user's apps.
const CPU_RAM_FRACTION: f64 = 0.50;
/// A full-GPU fit with ≥15% headroom reads "comfortable"; below that, "tight".
const COMFORTABLE_HEADROOM: f64 = 0.15;
/// Partial offload is only worth recommending above this GPU fraction.
const MIN_GPU_OFFLOAD: f64 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InferenceMode {
    GpuFull,
    CpuOnly,
}

/// How a model would actually run on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FitMode {
    /// Fully on the GPU(s) with comfortable headroom.
    GpuFull,
    /// Fully on the GPU(s) but with little room — hold context at 8K.
    GpuTight,
    /// Split across GPU and CPU/RAM — runs, but slower than full GPU.
    PartialOffload,
    /// Entirely on the CPU in system RAM — usable for drafts, not conversation.
    Cpu,
    /// Won't fit even in system RAM.
    Exceeds,
}

impl FitMode {
    /// Ranking tier for "best pick" selection (higher = better experience).
    fn tier(self) -> u8 {
        match self {
            FitMode::GpuFull => 4,
            FitMode::GpuTight => 3,
            FitMode::PartialOffload => 2,
            FitMode::Cpu => 1,
            FitMode::Exceeds => 0,
        }
    }
    fn runnable(self) -> bool {
        self != FitMode::Exceeds
    }
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
    /// Memory at the default (8K) context.
    pub est_mem_gb: f64,
    pub headroom_gb: f64,
    pub headroom_pct: f64,
    pub fit_mode: FitMode,
    /// For PartialOffload: fraction of weights on GPU, 0–100.
    pub gpu_offload_pct: Option<u32>,
    /// Largest context that fits fully on the GPU(s) (0 when not GPU-resident).
    pub max_ctx: u32,
    pub note: String,
}

/// Fit verdict for a single catalog quant — the UI's single source of truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuantFit {
    pub entry_id: String,
    pub quant: String,
    pub fit_mode: FitMode,
    pub est_mem_gb: f64,
    pub gpu_offload_pct: Option<u32>,
    pub max_ctx: u32,
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
    /// Usable GPU budget (GiB) at the default context — sum across eligible GPUs.
    pub budget_gb: f64,
    /// Usable system-RAM budget (GiB) for CPU offload.
    pub ram_budget_gb: f64,
    pub gpu_count: usize,
    pub multi_gpu: bool,
    /// VRAM already in use at detection (GiB) — subtracted from the budget.
    pub vram_in_use_gb: f64,
    pub default_ctx: u32,
    pub best: Option<Pick>,
    pub alternates: Vec<Pick>,
    pub by_role: Vec<RolePick>,
    /// Fit verdict for every catalog quant, at the default context.
    pub fits: Vec<QuantFit>,
    pub notes: Vec<String>,
}

/// The machine's usable budgets, derived once.
#[derive(Debug, Clone, Copy)]
struct Budget {
    gpu_gb: f64,
    ram_gb: f64,
    gpu_count: usize,
    vram_in_use_gb: f64,
}

fn budget_for(report: &HardwareReport) -> Budget {
    // Only CUDA-capable NVIDIA GPUs count toward the GPU budget (the runtime
    // path). Each contributes 95% of its total minus whatever it's already
    // holding — the display GPU's DWM/browser usage is real and subtracted.
    let mut gpu_gb = 0.0;
    let mut gpu_count = 0;
    let mut in_use = 0.0;
    for g in &report.gpus {
        if g.vendor != GpuVendor::Nvidia {
            continue;
        }
        let Some(total) = g.vram_total_bytes else { continue };
        let total_gb = total as f64 / GIB;
        if total_gb < 2.0 {
            continue;
        }
        let used_gb = g.vram_used_bytes.map(|b| b as f64 / GIB).unwrap_or(0.0);
        in_use += used_gb;
        gpu_gb += (total_gb * VRAM_USABLE_FRACTION - used_gb).max(0.0);
        gpu_count += 1;
    }
    Budget {
        gpu_gb,
        ram_gb: report.ram_gb() * CPU_RAM_FRACTION,
        gpu_count,
        vram_in_use_gb: in_use,
    }
}

/// (weights_gb, kv_per_token_gb, overhead_gb) for a quant.
fn mem_model(q: &QuantOption) -> (f64, f64, f64) {
    let weights = q.file_gb;
    let kv_at_ref = (q.min_mem_gb - weights - OVERHEAD_GB).max(0.0);
    (weights, kv_at_ref / KV_REF_CTX, OVERHEAD_GB)
}

fn mem_at_ctx(q: &QuantOption, ctx: u32) -> f64 {
    let (w, kv, ov) = mem_model(q);
    w + ov + kv * ctx as f64
}

/// Largest context (capped at the model's window) that fits fully on the GPU.
fn max_gpu_ctx(entry: &CatalogEntry, q: &QuantOption, gpu_gb: f64) -> u32 {
    let (w, kv, ov) = mem_model(q);
    if kv <= f64::EPSILON {
        return entry.context_length;
    }
    let room = gpu_gb - w - ov;
    if room <= 0.0 {
        return 0;
    }
    ((room / kv) as u32).min(entry.context_length)
}

struct Evaluation {
    fit_mode: FitMode,
    est_mem_gb: f64,
    gpu_offload_pct: Option<u32>,
    max_ctx: u32,
}

fn evaluate(entry: &CatalogEntry, q: &QuantOption, b: &Budget, ctx: u32) -> Evaluation {
    let mem = mem_at_ctx(q, ctx);
    let (weights, kv, ov) = mem_model(q);
    let kv_at_ctx = kv * ctx as f64;

    // Fully on the GPU?
    if b.gpu_gb > 0.0 && mem <= b.gpu_gb {
        let headroom_frac = (b.gpu_gb - mem) / b.gpu_gb;
        let mode = if headroom_frac >= COMFORTABLE_HEADROOM {
            FitMode::GpuFull
        } else {
            FitMode::GpuTight
        };
        return Evaluation {
            fit_mode: mode,
            est_mem_gb: mem,
            gpu_offload_pct: None,
            max_ctx: max_gpu_ctx(entry, q, b.gpu_gb),
        };
    }

    // Partial offload: some weight layers on the GPU, the rest in RAM. Feasible
    // only if the whole thing fits in RAM (worst case runs on CPU) and the GPU
    // can hold a worthwhile share after overhead + KV.
    let fits_in_ram = mem <= b.ram_gb;
    if b.gpu_gb > 0.0 && fits_in_ram {
        let gpu_for_weights = b.gpu_gb - ov - kv_at_ctx;
        if gpu_for_weights > 0.0 && weights > 0.0 {
            let frac = (gpu_for_weights / weights).clamp(0.0, 1.0);
            if frac >= MIN_GPU_OFFLOAD {
                return Evaluation {
                    fit_mode: FitMode::PartialOffload,
                    est_mem_gb: mem,
                    gpu_offload_pct: Some((frac * 100.0).round() as u32),
                    max_ctx: 0,
                };
            }
        }
    }

    // Pure CPU.
    if mem <= b.ram_gb {
        return Evaluation {
            fit_mode: FitMode::Cpu,
            est_mem_gb: mem,
            gpu_offload_pct: None,
            max_ctx: 0,
        };
    }

    Evaluation {
        fit_mode: FitMode::Exceeds,
        est_mem_gb: mem,
        gpu_offload_pct: None,
        max_ctx: 0,
    }
}

/// The best runnable quant for an entry: prefer the highest fit tier, then the
/// highest-quality quantization within that tier.
fn best_quant<'a>(entry: &'a CatalogEntry, b: &Budget, ctx: u32) -> Option<(&'a QuantOption, Evaluation)> {
    entry
        .quants
        .iter()
        .map(|q| (q, evaluate(entry, q, b, ctx)))
        .filter(|(_, e)| e.fit_mode.runnable())
        .max_by(|(qa, ea), (qb, eb)| {
            ea.fit_mode
                .tier()
                .cmp(&eb.fit_mode.tier())
                .then(qa.min_mem_gb.total_cmp(&qb.min_mem_gb))
        })
}

fn fit_note(entry: &CatalogEntry, q: &QuantOption, eval: &Evaluation, b: &Budget) -> String {
    match eval.fit_mode {
        FitMode::GpuFull => {
            let headroom = b.gpu_gb - eval.est_mem_gb;
            let ctx_note = if eval.max_ctx >= entry.context_length {
                format!("full {} context", ctx_human(entry.context_length))
            } else {
                format!("up to {} context", ctx_human(eval.max_ctx))
            };
            format!("Runs fully on the GPU with {headroom:.1} GB to spare — {ctx_note}.")
        }
        FitMode::GpuTight => format!(
            "Fits on the GPU with little headroom ({:.1} GB) — keep context near 8K.",
            b.gpu_gb - eval.est_mem_gb
        ),
        FitMode::PartialOffload => format!(
            "Splits across GPU and RAM (~{}% of layers on the GPU) — runs, but slower than a full-GPU fit.",
            eval.gpu_offload_pct.unwrap_or(0)
        ),
        FitMode::Cpu => "Runs on the CPU in system RAM — a few tokens/sec, fine for drafts.".into(),
        FitMode::Exceeds => "Exceeds this machine.".into(),
    }
    .replace("{q}", &q.label)
}

fn make_pick(entry: &CatalogEntry, b: &Budget, ctx: u32) -> Option<Pick> {
    let (quant, eval) = best_quant(entry, b, ctx)?;
    let headroom_gb = b.gpu_gb - eval.est_mem_gb;
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
        est_mem_gb: eval.est_mem_gb,
        headroom_gb,
        headroom_pct: if b.gpu_gb > 0.0 {
            (headroom_gb / b.gpu_gb * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        },
        fit_mode: eval.fit_mode,
        gpu_offload_pct: eval.gpu_offload_pct,
        max_ctx: eval.max_ctx,
        note: fit_note(entry, quant, &eval, b),
    })
}

fn ctx_human(tokens: u32) -> String {
    if tokens >= 1024 {
        format!("{}K", (tokens as f64 / 1024.0).round() as u32)
    } else {
        tokens.to_string()
    }
}

/// Order picks by fit tier, then quality — the "best experience" ranking.
fn rank(a: &Pick, b: &Pick) -> std::cmp::Ordering {
    b.fit_mode
        .tier()
        .cmp(&a.fit_mode.tier())
        .then(b.quality.cmp(&a.quality))
}

pub fn recommend(report: &HardwareReport) -> Result<RecommendationSet> {
    let cat = catalog()?;
    let b = budget_for(report);
    let ctx = DEFAULT_CTX;
    let mode = if b.gpu_gb >= 2.0 {
        InferenceMode::GpuFull
    } else {
        InferenceMode::CpuOnly
    };

    // Chat-capable picks, ranked by fit tier then quality.
    let mut picks: Vec<Pick> = cat
        .entries
        .iter()
        .filter(|e| !e.roles.contains(&Role::Embedding))
        .filter_map(|e| make_pick(e, &b, ctx))
        .collect();
    picks.sort_by(rank);

    let best = picks.first().cloned();
    let alternates: Vec<Pick> = picks.iter().skip(1).take(3).cloned().collect();

    let mut by_role = Vec::new();
    for role in [Role::General, Role::Coding, Role::Reasoning, Role::Embedding] {
        let role_best = cat
            .entries
            .iter()
            .filter(|e| e.roles.contains(&role))
            .filter_map(|e| make_pick(e, &b, ctx))
            .max_by(|x, y| rank(y, x));
        if let Some(pick) = role_best {
            by_role.push(RolePick { role, pick });
        }
    }

    // Fit verdict for every catalog quant — one source of truth for the UI.
    let mut fits = Vec::new();
    for entry in &cat.entries {
        for q in &entry.quants {
            let e = evaluate(entry, q, &b, ctx);
            fits.push(QuantFit {
                entry_id: entry.id.clone(),
                quant: q.label.clone(),
                fit_mode: e.fit_mode,
                est_mem_gb: e.est_mem_gb,
                gpu_offload_pct: e.gpu_offload_pct,
                max_ctx: e.max_ctx,
            });
        }
    }

    let mut notes = Vec::new();
    if b.gpu_gb >= 2.0 {
        if b.gpu_count > 1 {
            notes.push(format!(
                "Budget: {:.1} GB usable across {} GPUs (tensor-split), 95% of each minus VRAM in use.",
                b.gpu_gb, b.gpu_count
            ));
        } else {
            notes.push(format!(
                "Budget: {:.1} GB usable of GPU VRAM (95% usable minus {:.1} GB already in use).",
                b.gpu_gb, b.vram_in_use_gb
            ));
        }
        notes.push(format!("Fit shown at {} context; each pick lists the largest context it can hold.", ctx_human(ctx)));
    } else {
        notes.push(
            "No CUDA GPU detected — recommendations target CPU inference in system RAM.".into(),
        );
    }
    if best.is_none() {
        notes.push(
            "This machine is below the floor for local chat models. Cloud-assisted mode is the honest recommendation.".into(),
        );
    }

    Ok(RecommendationSet {
        mode,
        compute_class: report.compute_class,
        budget_gb: b.gpu_gb,
        ram_budget_gb: b.ram_gb,
        gpu_count: b.gpu_count,
        multi_gpu: b.gpu_count > 1,
        vram_in_use_gb: b.vram_in_use_gb,
        default_ctx: ctx,
        best,
        alternates,
        by_role,
        fits,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{classify, CpuInfo, GpuInfo, GpuVendor, MemoryInfo, OsInfo};

    fn gpu(vram_gb: f64, used_gb: f64) -> GpuInfo {
        const G: f64 = 1024.0 * 1024.0 * 1024.0;
        GpuInfo {
            name: "Test GPU".into(),
            vendor: GpuVendor::Nvidia,
            vram_total_bytes: Some((vram_gb * G) as u64),
            vram_used_bytes: Some((used_gb * G) as u64),
            driver_version: None,
            cuda_version: None,
            architecture: None,
            compute_capability: None,
            temperature_c: None,
            utilization_pct: None,
            source: "test".into(),
        }
    }

    fn report(gpus: Vec<GpuInfo>, ram_gb: f64) -> HardwareReport {
        const G: f64 = 1024.0 * 1024.0 * 1024.0;
        let max_vram = gpus
            .iter()
            .filter_map(|g| g.vram_total_bytes)
            .max()
            .map(|b| b as f64 / G)
            .unwrap_or(0.0);
        HardwareReport {
            cpu: CpuInfo {
                brand: "Test CPU".into(),
                physical_cores: Some(8),
                logical_cores: 16,
                base_frequency_mhz: 3600,
                arch: "x86_64".into(),
            },
            memory: MemoryInfo {
                total_bytes: (ram_gb * G) as u64,
                available_bytes: (ram_gb * G * 0.7) as u64,
            },
            gpus,
            disks: vec![],
            os: OsInfo {
                name: "Windows".into(),
                version: "11".into(),
                hostname: "test".into(),
                arch: "x86_64".into(),
            },
            compute_class: classify(max_vram),
            detected_at: "2026-07-03T00:00:00Z".into(),
        }
    }

    #[test]
    fn workstation_gets_the_flagship() {
        let recs = recommend(&report(vec![gpu(96.0, 0.0)], 256.0)).unwrap();
        let best = recs.best.expect("96GB best pick");
        assert_eq!(best.entry_id, "llama-3.3-70b-instruct");
        assert_eq!(best.fit_mode, FitMode::GpuFull);
        assert!(best.max_ctx >= 32768, "big card should hold long context, got {}", best.max_ctx);
    }

    #[test]
    fn dual_gpu_is_summed_for_tensor_split() {
        // Tony's persona: 2×24GB. Summed 48GB budget must fit the 70B, which a
        // single 24GB card cannot — the mis-scoping the assessment flagged.
        let recs = recommend(&report(vec![gpu(24.0, 0.0), gpu(24.0, 0.0)], 128.0)).unwrap();
        assert!(recs.multi_gpu);
        assert_eq!(recs.gpu_count, 2);
        assert!(recs.budget_gb > 44.0, "two 24GB cards should sum past 44GB, got {}", recs.budget_gb);
        let best = recs.best.unwrap();
        assert_eq!(best.entry_id, "llama-3.3-70b-instruct");
    }

    #[test]
    fn in_use_vram_is_subtracted() {
        // 24GB card already holding 10GB → only ~13GB usable; must NOT pick a
        // 32B that needs ~22GB.
        let recs = recommend(&report(vec![gpu(24.0, 10.0)], 64.0)).unwrap();
        assert!(recs.vram_in_use_gb >= 9.5);
        let best = recs.best.unwrap();
        assert!(
            best.est_mem_gb <= recs.budget_gb || best.fit_mode == FitMode::PartialOffload,
            "must not claim a full-GPU fit it can't honor: {} in {}",
            best.est_mem_gb,
            recs.budget_gb
        );
    }

    #[test]
    fn partial_offload_for_a_near_fit() {
        // 12GB card + 64GB RAM: a 32B (~22GB) can't fully fit but can split.
        let recs = recommend(&report(vec![gpu(12.0, 0.0)], 64.0)).unwrap();
        let has_partial = recs
            .fits
            .iter()
            .any(|f| f.entry_id == "qwen3-32b" && f.fit_mode == FitMode::PartialOffload);
        assert!(has_partial, "a 32B on a 12GB card should be a partial-offload candidate");
        let po = recs.fits.iter().find(|f| f.fit_mode == FitMode::PartialOffload);
        if let Some(f) = po {
            assert!(f.gpu_offload_pct.is_some());
        }
    }

    #[test]
    fn tight_is_never_over_budget() {
        // Every GpuTight verdict must genuinely fit within the GPU budget.
        for used in [0.0, 4.0, 9.0] {
            let recs = recommend(&report(vec![gpu(24.0, used)], 64.0)).unwrap();
            for f in &recs.fits {
                if f.fit_mode == FitMode::GpuTight {
                    assert!(
                        f.est_mem_gb <= recs.budget_gb + 1e-6,
                        "GpuTight {} {} is over budget {} > {}",
                        f.entry_id,
                        f.quant,
                        f.est_mem_gb,
                        recs.budget_gb
                    );
                }
            }
        }
    }

    #[test]
    fn context_scales_memory() {
        let cat = catalog().unwrap();
        let q = &cat.entries.iter().find(|e| e.id == "llama-3.3-70b-instruct").unwrap().quants[0];
        let at8k = mem_at_ctx(q, 8192);
        let at64k = mem_at_ctx(q, 65536);
        assert!(at64k > at8k + 5.0, "64K KV must cost meaningfully more than 8K");
    }

    #[test]
    fn cpu_only_machine_still_recommends() {
        let recs = recommend(&report(vec![], 32.0)).unwrap();
        assert_eq!(recs.mode, InferenceMode::CpuOnly);
        let best = recs.best.expect("32GB RAM runs something on CPU");
        assert_eq!(best.fit_mode, FitMode::Cpu);
    }

    #[test]
    fn tiny_machine_degrades_honestly() {
        let recs = recommend(&report(vec![], 4.0)).unwrap();
        assert!(recs.best.is_none());
        assert!(recs.notes.iter().any(|n| n.contains("below the floor")));
    }

    #[test]
    fn role_picks_cover_all_roles() {
        let recs = recommend(&report(vec![gpu(48.0, 0.0)], 128.0)).unwrap();
        assert_eq!(recs.by_role.len(), 4);
    }

    #[test]
    fn every_quant_has_a_fit_verdict() {
        let recs = recommend(&report(vec![gpu(24.0, 0.0)], 64.0)).unwrap();
        let cat = catalog().unwrap();
        let quant_count: usize = cat.entries.iter().map(|e| e.quants.len()).sum();
        assert_eq!(recs.fits.len(), quant_count);
    }
}
