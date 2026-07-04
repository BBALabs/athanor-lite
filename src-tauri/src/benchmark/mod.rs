//! Speed benchmark — measure how fast a model actually runs on *this* machine.
//! A fixed suite of short prompts is generated with a capped output; the real
//! timings llama-server reports (TTFT, prompt eval, generation tok/s) are
//! averaged and saved. No synthetic scores — every number is measured on the
//! user's own hardware.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::ops::{OpKind, Ops};
use crate::runtime::server::Llm;
use crate::workspaces::{self, write_atomic};

/// Short, varied prompts. Kept small so a run is quick but still exercises both
/// prompt processing and generation.
const SUITE: &[&str] = &[
    "Write a haiku about the ocean.",
    "List the first eight prime numbers.",
    "Explain what a hash map is in two sentences.",
    "Summarize the water cycle in three short bullet points.",
];
const MAX_TOKENS: u32 = 160;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchResult {
    #[serde(default = "one")]
    pub schema: u32,
    pub model_sha: String,
    pub model_name: String,
    /// Averages across the suite.
    pub ttft_ms: u64,
    pub gen_tps: f64,
    pub prompt_tps: f64,
    pub gpu_active: bool,
    pub prompts: usize,
    pub ran_at: String,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BenchFile {
    #[serde(default)]
    results: Vec<BenchResult>,
}

fn bench_path(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(workspaces::data_root(app)?.join("benchmarks.json"))
}

fn read_file(app: &tauri::AppHandle) -> BenchFile {
    match std::fs::read(bench_path(app).unwrap_or_default()) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or_default(),
        Err(_) => BenchFile::default(),
    }
}

/// The leaderboard — one result per model, fastest generation first.
pub fn list(app: &tauri::AppHandle) -> Result<Vec<BenchResult>> {
    let mut r = read_file(app).results;
    r.sort_by(|a, b| b.gen_tps.partial_cmp(&a.gen_tps).unwrap_or(std::cmp::Ordering::Equal));
    Ok(r)
}

/// Run the suite against a model and save the averaged result (replacing any
/// prior run for that model). Registered as a cancellable operation.
pub fn run(
    app: &tauri::AppHandle,
    llm: &Llm,
    ops: &Ops,
    model_sha: &str,
    model_name: &str,
) -> Result<BenchResult> {
    use std::sync::atomic::Ordering;
    let cancel = ops
        .begin(
            app,
            "benchmark",
            OpKind::Benchmark,
            &format!("Benchmarking {model_name}"),
            true,
            None,
        )
        .ok_or_else(|| crate::error::AthanorError::Chat("a benchmark is already running".into()))?;

    let mut ttft_sum = 0u64;
    let mut gen_sum = 0.0f64;
    let mut prompt_sum = 0.0f64;
    let mut n = 0u64;
    let mut gpu_active = false;

    let result = (|| -> Result<()> {
        for (i, prompt) in SUITE.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            ops.progress(app, "benchmark", i as u64, SUITE.len() as u64, &format!("prompt {}/{}", i + 1, SUITE.len()));
            let stats = crate::chat::measure(app, llm, model_sha, prompt, MAX_TOKENS)?;
            ttft_sum += stats.ttft_ms;
            gen_sum += stats.predicted_per_second;
            prompt_sum += stats.prompt_per_second;
            gpu_active = stats.gpu_active;
            n += 1;
        }
        Ok(())
    })();

    match &result {
        Ok(()) if cancel.load(Ordering::Relaxed) => ops.cancelled(app, "benchmark"),
        Ok(()) => ops.done(app, "benchmark"),
        Err(e) => ops.failed(app, "benchmark", &e.to_string()),
    }
    result?;
    if n == 0 {
        return Err(crate::error::AthanorError::Chat("benchmark cancelled before any result".into()));
    }

    let res = BenchResult {
        schema: 1,
        model_sha: model_sha.to_string(),
        model_name: model_name.to_string(),
        ttft_ms: ttft_sum / n,
        gen_tps: gen_sum / n as f64,
        prompt_tps: prompt_sum / n as f64,
        gpu_active,
        prompts: n as usize,
        ran_at: chrono::Utc::now().to_rfc3339(),
    };

    // Persist — one row per model (latest wins).
    let mut file = read_file(app);
    file.results.retain(|r| r.model_sha != res.model_sha);
    file.results.push(res.clone());
    write_atomic(&bench_path(app)?, &serde_json::to_vec_pretty(&file)?)?;

    log::info!(
        target: "bench",
        "benchmarked {model_name}: {:.1} tok/s gen, {} ms ttft ({} prompts)",
        res.gen_tps, res.ttft_ms, res.prompts
    );
    Ok(res)
}
