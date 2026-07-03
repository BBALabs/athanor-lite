/**
 * System dashboard — the first screen. Left: the crest (live pressure gauge +
 * compute class) and the recommended loadout. Right: instrument strips for
 * CPU, memory, GPUs, and storage, all fed by the 1 Hz telemetry stream.
 */

import { useStore, useLatestSample } from "../state/store";
import { ArcGauge } from "../components/ArcGauge";
import { SegmentBar } from "../components/SegmentBar";
import { Sparkline } from "../components/Sparkline";
import { StatusPill } from "../components/StatusPill";
import { BoltIcon, ChipIcon, DiskIcon, GpuIcon, RamIcon } from "../components/Icons";
import {
  bytesHuman,
  COMPUTE_CLASS_LABEL,
  ghz,
  gib,
  loadColor,
  pct,
} from "../lib/format";
import type { Pick as ModelPick } from "../lib/types";

function LoadoutPick({ pick, prime }: { pick: ModelPick; prime?: boolean }) {
  const budget = useStore((s) => s.recommendations?.budgetGb ?? 0);
  const fitFrac = budget > 0 ? pick.estMemGb / budget : 0;

  if (!prime) {
    return (
      <div className="loadout__alt">
        <span className="loadout__alt-name">{pick.name}</span>
        <span className="k-chip">{pick.quant}</span>
        <span className="k-num loadout__alt-mem">{pick.estMemGb.toFixed(1)} GB</span>
      </div>
    );
  }

  return (
    <div className="loadout__prime">
      <div className="loadout__prime-head">
        <BoltIcon size={15} className="loadout__bolt" />
        <span className="k-label">best single model</span>
      </div>
      <div className="loadout__prime-name">{pick.name}</div>
      <div className="loadout__prime-chips">
        <span className="k-chip">{pick.paramsB.toFixed(0)}B params</span>
        <span className="k-chip">{pick.quant}</span>
        <span className="k-chip">{pick.fileGb.toFixed(1)} GB download</span>
      </div>
      <div className="loadout__fit">
        <SegmentBar value={fitFrac} segments={24} height={10} />
        <div className="loadout__fit-caption k-num">
          {pick.estMemGb.toFixed(1)} / {budget.toFixed(1)} GB budget
        </div>
      </div>
      <p className="loadout__note">{pick.note}</p>
    </div>
  );
}

export function Dashboard() {
  const hw = useStore((s) => s.hardware);
  const recs = useStore((s) => s.recommendations);
  const telemetry = useStore((s) => s.telemetry);
  const sample = useLatestSample();

  if (!hw) return null;

  const cls = COMPUTE_CLASS_LABEL[hw.computeClass] ?? COMPUTE_CLASS_LABEL.CpuOnly;
  const primaryGpu = hw.gpus[0] ?? null;
  const liveGpu = sample?.gpus[0] ?? null;

  // Crest gauge: VRAM pressure when we have a live GPU feed, else RAM pressure.
  const crest = liveGpu
    ? {
        frac: liveGpu.vramUsedBytes / Math.max(1, liveGpu.vramTotalBytes),
        big: gib(liveGpu.vramUsedBytes, 1),
        of: `${gib(liveGpu.vramTotalBytes)} GB VRAM`,
        label: "vram pressure",
      }
    : {
        frac: sample ? sample.memUsedBytes / Math.max(1, sample.memTotalBytes) : 0,
        big: sample ? gib(sample.memUsedBytes) : "—",
        of: `${gib(hw.memory.totalBytes)} GB RAM`,
        label: "memory pressure",
      };

  const cpuTrace = telemetry.map((t) => t.cpuUsagePct);
  const memFrac = sample ? sample.memUsedBytes / Math.max(1, sample.memTotalBytes) : 0;

  return (
    <div className="dash">
      <div className="dash__head">
        <div>
          <h1 className="dash__title">System</h1>
          <div className="dash__sub k-num">
            {hw.os.hostname} · {hw.os.name} {hw.os.version} · {hw.os.arch}
          </div>
        </div>
        <div className="dash__pills">
          <StatusPill tone={sample ? "ok" : "idle"} label={sample ? "telemetry live" : "telemetry warming"} live={!!sample} />
          <StatusPill
            tone={primaryGpu?.source === "nvml" ? "ok" : primaryGpu ? "info" : "warn"}
            label={primaryGpu ? (primaryGpu.source === "nvml" ? "nvml probe" : "wmi probe") : "no gpu"}
          />
        </div>
      </div>

      <div className="dash__grid">
        {/* ── Crest column ─────────────────────────────── */}
        <section className="panel crest" style={{ ["--stagger" as string]: 0 }}>
          <ArcGauge value={crest.frac} size={230}>
            <div className="crest__big k-num" style={{ color: loadColor(crest.frac) }}>
              {crest.big}
              <span className="crest__unit">GB</span>
            </div>
            <div className="crest__of k-num">of {crest.of}</div>
            <div className="crest__gauge-label k-label">{crest.label}</div>
          </ArcGauge>

          <div className="crest__class">
            <div className="crest__class-title">{cls.title}</div>
            <div className="crest__class-sub">{cls.sub}</div>
          </div>

          {primaryGpu && (
            <div className="crest__gpu">
              <div className="crest__gpu-name">{primaryGpu.name}</div>
              <div className="crest__gpu-chips">
                {primaryGpu.driverVersion && <span className="k-chip">driver {primaryGpu.driverVersion}</span>}
                {primaryGpu.cudaVersion && <span className="k-chip">CUDA {primaryGpu.cudaVersion}</span>}
              </div>
            </div>
          )}
        </section>

        {/* ── Loadout column ───────────────────────────── */}
        <section className="panel loadout" style={{ ["--stagger" as string]: 1 }}>
          <div className="panel__title">
            <span className="k-label">recommended loadout</span>
            {recs && (
              <span className="k-chip">
                budget {recs.budgetGb.toFixed(1)} GB · {recs.mode === "cpuOnly" ? "cpu inference" : "gpu inference"}
              </span>
            )}
          </div>

          {recs?.best ? (
            <>
              <LoadoutPick pick={recs.best} prime />
              {recs.alternates.length > 0 && (
                <div className="loadout__alts">
                  <div className="k-label loadout__alts-label">also fits</div>
                  {recs.alternates.map((p) => (
                    <LoadoutPick key={p.entryId} pick={p} />
                  ))}
                </div>
              )}
              {recs.byRole.length > 0 && (
                <div className="loadout__roles">
                  {recs.byRole.map((rp) => (
                    <div key={rp.role} className="loadout__role">
                      <span className="k-label">{rp.role}</span>
                      <span className="loadout__role-name">{rp.pick.name}</span>
                    </div>
                  ))}
                </div>
              )}
            </>
          ) : (
            <div className="loadout__empty">
              <p>{recs?.notes.join(" ") ?? "No recommendation available."}</p>
            </div>
          )}
        </section>

        {/* ── Instrument strips ────────────────────────── */}
        <div className="strips">
          <section className="panel strip" style={{ ["--stagger" as string]: 2 }}>
            <div className="strip__icon"><ChipIcon /></div>
            <div className="strip__body">
              <div className="strip__row">
                <span className="strip__name">{hw.cpu.brand}</span>
                <span className="k-num strip__value" style={{ color: loadColor((sample?.cpuUsagePct ?? 0) / 100) }}>
                  {sample ? pct(sample.cpuUsagePct) : "—"}
                </span>
              </div>
              <div className="strip__row strip__row--meta">
                <span className="k-chip">{hw.cpu.physicalCores ?? "?"}C / {hw.cpu.logicalCores}T</span>
                {hw.cpu.baseFrequencyMhz > 0 && <span className="k-chip">{ghz(hw.cpu.baseFrequencyMhz)}</span>}
                <div className="strip__spark">
                  <Sparkline data={cpuTrace} max={100} width={220} height={40} />
                </div>
              </div>
            </div>
          </section>

          <section className="panel strip" style={{ ["--stagger" as string]: 3 }}>
            <div className="strip__icon"><RamIcon /></div>
            <div className="strip__body">
              <div className="strip__row">
                <span className="strip__name">Memory</span>
                <span className="k-num strip__value">
                  {sample ? `${gib(sample.memUsedBytes)} / ${gib(sample.memTotalBytes)} GB` : `${gib(hw.memory.totalBytes)} GB`}
                </span>
              </div>
              <SegmentBar value={memFrac} segments={36} />
            </div>
          </section>

          {hw.gpus.map((gpu, i) => {
            const live = sample?.gpus.find((g) => g.name === gpu.name) ?? (i === 0 ? liveGpu : null);
            const frac = live
              ? live.vramUsedBytes / Math.max(1, live.vramTotalBytes)
              : 0;
            return (
              <section className="panel strip" key={`${gpu.name}-${i}`} style={{ ["--stagger" as string]: 4 + i }}>
                <div className="strip__icon"><GpuIcon /></div>
                <div className="strip__body">
                  <div className="strip__row">
                    <span className="strip__name">{gpu.name}</span>
                    <span className="k-num strip__value">
                      {live
                        ? `${gib(live.vramUsedBytes, 1)} / ${gib(live.vramTotalBytes)} GB`
                        : gpu.vramTotalBytes
                          ? `${gib(gpu.vramTotalBytes)} GB VRAM`
                          : "VRAM unknown"}
                    </span>
                  </div>
                  {live ? (
                    <>
                      <SegmentBar value={frac} segments={36} />
                      <div className="strip__row strip__row--meta">
                        <span className="k-chip">util {pct(live.utilizationPct)}</span>
                        <span className="k-chip">{live.temperatureC}°C</span>
                      </div>
                    </>
                  ) : (
                    <div className="strip__row strip__row--meta">
                      <span className="k-chip">static probe — live stats need an NVIDIA driver</span>
                    </div>
                  )}
                </div>
              </section>
            );
          })}

          <section className="panel strip" style={{ ["--stagger" as string]: 5 + hw.gpus.length }}>
            <div className="strip__icon"><DiskIcon /></div>
            <div className="strip__body">
              <div className="strip__row">
                <span className="strip__name">Storage</span>
                <span className="k-num strip__value">
                  {bytesHuman(hw.disks.reduce((a, d) => a + d.availableBytes, 0))} free
                </span>
              </div>
              <div className="strip__disks">
                {hw.disks.map((d, i) => {
                  const used = 1 - d.availableBytes / Math.max(1, d.totalBytes);
                  return (
                    <div className="disk" key={`${d.mount}-${i}`}>
                      <span className="disk__mount k-num">{d.mount}</span>
                      <div className="disk__bar">
                        <div
                          className="disk__fill"
                          style={{ width: `${(used * 100).toFixed(1)}%`, background: loadColor(used) }}
                        />
                      </div>
                      <span className="disk__free k-num">{bytesHuman(d.availableBytes)} free</span>
                    </div>
                  );
                })}
              </div>
            </div>
          </section>
        </div>
      </div>

      {recs && recs.notes.length > 0 && (
        <div className="dash__notes">
          {recs.notes.map((n, i) => (
            <span key={i} className="k-chip dash__note">{n}</span>
          ))}
        </div>
      )}
    </div>
  );
}
