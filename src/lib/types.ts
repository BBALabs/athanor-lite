/**
 * TypeScript mirrors of the Rust IPC structs (serde camelCase).
 * Kept in lockstep with src-tauri/src — change both sides together.
 */

export type GpuVendor = "Nvidia" | "Amd" | "Intel" | "Other";

export type ComputeClass =
  | "CpuOnly"
  | "VramLow"
  | "VramMid"
  | "VramHigh"
  | "VramWorkstation";

export interface CpuInfo {
  brand: string;
  physicalCores: number | null;
  logicalCores: number;
  baseFrequencyMhz: number;
  arch: string;
}

export interface MemoryInfo {
  totalBytes: number;
  availableBytes: number;
}

export interface GpuInfo {
  name: string;
  vendor: GpuVendor;
  vramTotalBytes: number | null;
  vramUsedBytes: number | null;
  driverVersion: string | null;
  cudaVersion: string | null;
  /** GPU generation ("Blackwell", "Ada Lovelace", …) from compute capability. */
  architecture: string | null;
  /** CUDA compute capability, e.g. "12.0". */
  computeCapability: string | null;
  temperatureC: number | null;
  utilizationPct: number | null;
  source: "nvml" | "wmi" | string;
}

export interface DiskInfo {
  name: string;
  mount: string;
  totalBytes: number;
  availableBytes: number;
  kind: string;
}

export interface OsInfo {
  name: string;
  version: string;
  hostname: string;
  arch: string;
}

export interface HardwareReport {
  cpu: CpuInfo;
  memory: MemoryInfo;
  gpus: GpuInfo[];
  disks: DiskInfo[];
  os: OsInfo;
  computeClass: ComputeClass;
  detectedAt: string;
}

export interface GpuTelemetry {
  index: number;
  name: string;
  vramTotalBytes: number;
  vramUsedBytes: number;
  utilizationPct: number;
  temperatureC: number;
}

export interface TelemetrySample {
  tsMs: number;
  cpuUsagePct: number;
  memTotalBytes: number;
  memUsedBytes: number;
  gpus: GpuTelemetry[];
}

export type Role = "general" | "coding" | "reasoning" | "embedding";

export interface QuantOption {
  label: string;
  fileGb: number;
  minMemGb: number;
}

export interface CatalogEntry {
  id: string;
  family: string;
  name: string;
  paramsB: number;
  roles: Role[];
  quality: number;
  contextLength: number;
  license: string;
  hfRepo: string;
  blurb: string;
  quants: QuantOption[];
}

export interface Catalog {
  version: string;
  entries: CatalogEntry[];
}

export type InferenceMode = "gpuFull" | "cpuOnly";

export interface Pick {
  entryId: string;
  name: string;
  family: string;
  paramsB: number;
  roles: Role[];
  quality: number;
  blurb: string;
  quant: string;
  fileGb: number;
  estMemGb: number;
  headroomGb: number;
  headroomPct: number;
  note: string;
}

export interface RolePick {
  role: Role;
  pick: Pick;
}

export interface RecommendationSet {
  mode: InferenceMode;
  computeClass: ComputeClass;
  budgetGb: number;
  best: Pick | null;
  alternates: Pick[];
  byRole: RolePick[];
  notes: string[];
}

export interface Workspace {
  id: string;
  name: string;
  purpose: string;
  accentHue: number;
  glyph: string;
  createdAt: string;
  lastOpenedAt: string;
  modelRefs: string[];
}

export interface WorkspaceList {
  workspaces: Workspace[];
  activeId: string | null;
}

export interface CondereError {
  code: string;
  message: string;
}

export function isCondereError(e: unknown): e is CondereError {
  return (
    typeof e === "object" &&
    e !== null &&
    "code" in e &&
    "message" in e &&
    typeof (e as CondereError).message === "string"
  );
}
