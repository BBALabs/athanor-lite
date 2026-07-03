/**
 * System — the instrument cluster. Zone A: machine identity · Power Ring ·
 * recommendation. Zone B: one telemetry band on a shared baseline. Zone C:
 * per-role picks as a single quiet line. Eye flow: ring → model → band.
 */

import { useStore, useLatestSample } from "../state/store";
import { PowerRing } from "../components/PowerRing";
import { LightLine } from "../components/LightLine";
import { Sparkline } from "../components/Sparkline";
import { useTweenedNumber } from "../lib/useTween";
import { bytesHuman, COMPUTE_CLASS_LABEL, ghz, gib } from "../lib/format";

const GIB = 1024 ** 3;

function BandCell({
  label,
  value,
  unit,
  sub,
  load,
  spark,
}: {
  label: string;
  value: string;
  unit?: string;
  sub?: string;
  load: number;
  spark?: number[];
}) {
  return (
    <div className="band__cell">
      <span className="t-label">{label}</span>
      <div className="band__value">
        <span className="t-readout">{value}</span>
        {unit && <span className="band__unit">{unit}</span>}
      </div>
      <span className="band__sub t-quiet">{sub ?? ""}</span>
      <LightLine value={load} />
      {spark && spark.length > 1 && (
        <span className="band__spark">
          <Sparkline data={spark} max={100} width={130} height={30} />
        </span>
      )}
    </div>
  );
}

export function Dashboard() {
  const hw = useStore((s) => s.hardware);
  const recs = useStore((s) => s.recommendations);
  const telemetry = useStore((s) => s.telemetry);
  const sample = useLatestSample();

  const liveGpu = sample?.gpus[0] ?? null;
  const primaryGpu = hw?.gpus[0] ?? null;

  // The ring shows the pressure that matters for AI work: VRAM when a live
  // GPU feed exists, system memory otherwise.
  const ringTargetFrac = liveGpu
    ? liveGpu.vramUsedBytes / Math.max(1, liveGpu.vramTotalBytes)
    : sample
      ? sample.memUsedBytes / Math.max(1, sample.memTotalBytes)
      : 0;
  const ringTargetGb = liveGpu
    ? liveGpu.vramUsedBytes / GIB
    : (sample?.memUsedBytes ?? 0) / GIB;

  const ringFrac = useTweenedNumber(ringTargetFrac, 900);
  const ringGb = useTweenedNumber(ringTargetGb, 900);
  const cpuPct = useTweenedNumber(sample?.cpuUsagePct ?? 0, 700);
  const memGb = useTweenedNumber((sample?.memUsedBytes ?? 0) / GIB, 900);
  const vramGb = useTweenedNumber((liveGpu?.vramUsedBytes ?? 0) / GIB, 900);
  const gpuUtil = useTweenedNumber(liveGpu?.utilizationPct ?? 0, 700);

  if (!hw) {
    // Degraded state: detection failed — the cabin stays open, honestly labeled.
    return (
      <div className="dash view">
        <header className="view-head">
          <h1 className="t-display">System</h1>
        </header>
        <section className="degraded">
          <div className="t-title">Hardware detection unavailable</div>
          <p className="t-quiet degraded__note">
            The probe could not read this machine. Model browsing still works;
            recommendations need a hardware profile. Details are in the log file.
          </p>
          <button className="btn-lit" onClick={() => void useStore.getState().retryHardware()}>
            Retry detection
          </button>
        </section>
      </div>
    );
  }

  const cls = COMPUTE_CLASS_LABEL[hw.computeClass] ?? COMPUTE_CLASS_LABEL.CpuOnly;
  const ringTotal = liveGpu
    ? gib(liveGpu.vramTotalBytes)
    : gib(hw.memory.totalBytes);
  const cpuTrace = telemetry.map((t) => t.cpuUsagePct);
  const memFrac = sample ? sample.memUsedBytes / Math.max(1, sample.memTotalBytes) : 0;

  return (
    <div className="dash view">
      <header className="view-head">
        <h1 className="t-display">System</h1>
        <span className="view-head__sub t-quiet">
          {hw.os.hostname} · {hw.os.name} {hw.os.version} · {hw.os.arch}
        </span>
      </header>

      {/* ── Zone A — the cluster ─────────────────────────── */}
      <section className="cluster">
        <div className="cluster__id">
          <span className="t-label">Machine</span>
          {primaryGpu && <div className="cluster__id-name">{primaryGpu.name}</div>}
          <div className="cluster__id-name cluster__id-name--dim">{hw.cpu.brand}</div>
          <div className="cluster__id-class t-quiet">
            {cls.title} · {hw.cpu.physicalCores ?? "?"} cores ·{" "}
            {gib(hw.memory.totalBytes)} GB memory
            {hw.cpu.baseFrequencyMhz > 0 ? ` · ${ghz(hw.cpu.baseFrequencyMhz)}` : ""}
          </div>
          {primaryGpu && (
            <div className="cluster__id-drv t-quiet">
              {primaryGpu.source === "nvml"
                ? [
                    primaryGpu.architecture,
                    primaryGpu.driverVersion ? `driver ${primaryGpu.driverVersion}` : null,
                    primaryGpu.cudaVersion ? `CUDA ${primaryGpu.cudaVersion}` : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")
                : "static probe — live GPU stats need an NVIDIA driver"}
            </div>
          )}
        </div>

        <div className="cluster__ring">
          <PowerRing value={ringFrac} size={340}>
            <span className="t-hero">
              {ringGb >= 100 ? ringGb.toFixed(0) : ringGb.toFixed(1)}
            </span>
            <span className="ring__of t-quiet">
              {liveGpu ? "VRAM" : "memory"} · of {ringTotal} GB
            </span>
          </PowerRing>
        </div>

        <div className="cluster__rec">
          <span className="t-label">Recommended</span>
          {recs?.best ? (
            <>
              <div className="cluster__rec-name t-display">{recs.best.name}</div>
              <div className="cluster__rec-fit t-quiet">
                <span className="t-mono">{recs.best.quant}</span> ·{" "}
                {recs.best.fileGb.toFixed(1)} GB download · runs in{" "}
                {recs.best.estMemGb.toFixed(1)} of {recs.budgetGb.toFixed(1)} GB
              </div>
              <p className="cluster__rec-note t-quiet">{recs.best.note}</p>
              {recs.alternates.length > 0 && (
                <div className="cluster__alts">
                  {recs.alternates.map((p) => (
                    <div key={p.entryId} className="cluster__alt">
                      <span className="cluster__alt-name">{p.name}</span>
                      <span className="t-mono">{p.quant}</span>
                    </div>
                  ))}
                </div>
              )}
            </>
          ) : (
            <p className="cluster__rec-note t-quiet">
              {recs?.notes.join(" ") ?? "No recommendation available for this machine."}
            </p>
          )}
        </div>
      </section>

      {/* ── Zone B — the telemetry band ──────────────────── */}
      <section className="band">
        <BandCell
          label="CPU"
          value={cpuPct.toFixed(0)}
          unit="%"
          sub={`${hw.cpu.physicalCores ?? "?"}C / ${hw.cpu.logicalCores}T`}
          load={cpuPct / 100}
          spark={cpuTrace}
        />
        <BandCell
          label="Memory"
          value={memGb.toFixed(1)}
          unit="GB"
          sub={`of ${gib(hw.memory.totalBytes)} GB`}
          load={memFrac}
        />
        {liveGpu && (
          <>
            <BandCell
              label="GPU"
              value={gpuUtil.toFixed(0)}
              unit="%"
              sub={`${liveGpu.temperatureC}°C`}
              load={gpuUtil / 100}
            />
            <BandCell
              label="VRAM"
              value={vramGb.toFixed(1)}
              unit="GB"
              sub={`of ${gib(liveGpu.vramTotalBytes)} GB`}
              load={liveGpu.vramUsedBytes / Math.max(1, liveGpu.vramTotalBytes)}
            />
          </>
        )}
        {hw.disks.slice(0, 3).map((d, i) => {
          const used = 1 - d.availableBytes / Math.max(1, d.totalBytes);
          return (
            <div className="band__cell band__cell--minor" key={`${d.mount}-${i}`}>
              <span className="t-label">{d.mount.replace(/\\$/, "")}</span>
              <div className="band__value">
                <span className="band__minor-value tnum">{bytesHuman(d.availableBytes)}</span>
              </div>
              <span className="band__sub t-quiet">free</span>
              <LightLine value={used} />
            </div>
          );
        })}
      </section>

      {/* ── Zone C — role picks ──────────────────────────── */}
      {recs && recs.byRole.length > 0 && (
        <section className="roles t-quiet">
          {recs.byRole.map((rp, i) => (
            <span key={rp.role} className="roles__pair">
              {i > 0 && <span className="roles__sep">·</span>}
              <span className="roles__role">{rp.role}</span>
              <span className="roles__model">{rp.pick.name}</span>
            </span>
          ))}
        </section>
      )}
    </div>
  );
}
