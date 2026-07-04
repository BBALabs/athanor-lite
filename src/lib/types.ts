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

export interface QuantFile {
  name: string;
  sizeBytes: number;
  sha256: string;
}

export interface QuantOption {
  label: string;
  fileGb: number;
  minMemGb: number;
  files: QuantFile[];
}

export type DownloadState =
  | "starting"
  | "downloading"
  | "verifying"
  | "done"
  | "failed"
  | "cancelled";

export interface DownloadProgress {
  sha256: string;
  entryId: string;
  quant: string;
  fileName: string;
  receivedBytes: number;
  totalBytes: number;
  bytesPerSec: number;
  state: DownloadState;
  error: string | null;
}

export interface LibraryModel {
  schema: number;
  sha256: string;
  fileName: string;
  path: string;
  sizeBytes: number;
  displayName: string;
  entryId: string | null;
  quant: string | null;
  source: string;
  addedAt: string;
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

export type FitMode = "gpuFull" | "gpuTight" | "partialOffload" | "cpu" | "exceeds";

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
  fitMode: FitMode;
  gpuOffloadPct: number | null;
  maxCtx: number;
  note: string;
}

export interface QuantFit {
  entryId: string;
  quant: string;
  fitMode: FitMode;
  estMemGb: number;
  gpuOffloadPct: number | null;
  maxCtx: number;
}

export interface RolePick {
  role: Role;
  pick: Pick;
}

export interface RecommendationSet {
  mode: InferenceMode;
  computeClass: ComputeClass;
  budgetGb: number;
  ramBudgetGb: number;
  gpuCount: number;
  multiGpu: boolean;
  vramInUseGb: number;
  defaultCtx: number;
  best: Pick | null;
  alternates: Pick[];
  byRole: RolePick[];
  fits: QuantFit[];
  notes: string[];
}

export interface Workspace {
  schema: number;
  id: string;
  name: string;
  purpose: string;
  accentHue: number;
  glyph: string;
  createdAt: string;
  lastOpenedAt: string;
  modelRefs: string[];
  /** Library model (sha256) this workspace chats with. */
  activeModel: string | null;
}

export interface WorkspaceList {
  workspaces: Workspace[];
  activeId: string | null;
  /** Workspace dirs whose manifests could not be read — surfaced, never hidden. */
  damaged: string[];
}

export interface GenStats {
  ttftMs: number;
  promptN: number;
  predictedN: number;
  promptPerSecond: number;
  predictedPerSecond: number;
  contextUsed: number;
  gpuActive: boolean;
  cancelled: boolean;
}

export interface Source {
  docId: string;
  docName: string;
  chunkIndex: number;
  score: number;
  excerpt: string;
}

export interface ToolStep {
  server: string;
  tool: string;
  arguments: string;
  result: string;
  ok: boolean;
}

export interface ChatMessage {
  role: "user" | "assistant" | string;
  content: string;
  ts: string;
  stats: GenStats | null;
  sources: Source[];
  toolSteps: ToolStep[];
}

export type DocStatus = "indexing" | "ready" | "failed";

export interface KbDocument {
  id: string;
  name: string;
  sourcePath: string;
  bytes: number;
  chunkCount: number;
  status: DocStatus;
  error: string | null;
  addedAt: string;
}

export interface KnowledgeBase {
  documents: KbDocument[];
  retrievalEnabled: boolean;
  chunkTotal: number;
}

export interface ChatRetrieval {
  workspaceId: string;
  conversationId: string;
  sources: Source[];
}

export interface ChatToolEvent {
  workspaceId: string;
  conversationId: string;
  step: ToolStep;
}

export interface McpTool {
  name: string;
  title: string | null;
  description: string | null;
}

export interface McpServerConfig {
  id: string;
  name: string;
  command: string;
  args: string[];
  env: Record<string, string>;
}

export interface McpServerView {
  config: McpServerConfig;
  connected: boolean;
  serverName: string | null;
  tools: McpTool[];
  error: string | null;
}

export interface Conversation {
  schema: number;
  id: string;
  workspaceId: string;
  title: string;
  modelSha: string | null;
  createdAt: string;
  updatedAt: string;
  messages: ChatMessage[];
}

export interface ConversationMeta {
  id: string;
  title: string;
  updatedAt: string;
  messageCount: number;
}

export interface ChatDelta {
  workspaceId: string;
  conversationId: string;
  delta: string;
}

export interface ChatDone {
  workspaceId: string;
  conversationId: string;
  content: string;
  stats: GenStats | null;
  error: string | null;
}

export interface RuntimeState {
  phase: "checking" | "downloading" | "extracting" | "ready" | "error" | string;
  backend: "cuda12" | "cuda13" | "cpu";
  tag: string;
  receivedBytes: number;
  totalBytes: number;
  detail: string;
}

export interface ServerStatus {
  phase: "starting" | "loading" | "ready" | "stopped" | "error" | string;
  modelSha: string | null;
  modelName: string | null;
  port: number | null;
  backend: "cuda12" | "cuda13" | "cpu" | null;
  gpuActive: boolean;
  vramAtLoadBytes: number | null;
  detail: string;
}

export interface MetricsSettings {
  schema: number;
  share: boolean;
}

/** Which contextual walkthroughs the user has already completed or dismissed. */
export interface CoachState {
  schema: number;
  seen: string[];
}

export interface MetricsRecord {
  schema: number;
  ts: string;
  event: string;
  hw: {
    gpu: string | null;
    vramGb: number | null;
    driver: string | null;
    cpu: string;
    ramGb: number;
    os: string;
  };
  llamaBuild: string;
  appVersion: string;
  modelSha: string;
  ctx: number;
  gpuActive: boolean;
  ttftMs: number | null;
  promptN: number | null;
  promptPerSecond: number | null;
  predictedN: number | null;
  predictedPerSecond: number | null;
  vramAtLoadBytes: number | null;
  errorKind: string | null;
}

export interface OllamaStatus {
  available: boolean;
  root: string | null;
  modelCount: number;
}

export interface ImportReport {
  found: number;
  imported: number;
  alreadyInLibrary: number;
  skipped: string[];
}

export interface ApiInfo {
  expose: boolean;
  running: boolean;
  baseUrl: string;
  apiKey: string;
  modelName: string | null;
}

export type OpKind =
  | "download"
  | "engineFetch"
  | "engine"
  | "generation"
  | "import"
  | "index"
  | "mcp";
export type OpState = "running" | "failed" | "cancelled";

export interface Operation {
  id: string;
  kind: OpKind;
  state: OpState;
  label: string;
  detail: string;
  progressCurrent: number | null;
  progressTotal: number | null;
  resourceNote: string | null;
  startedAt: string;
  error: string | null;
  cancellable: boolean;
  retry: { kind: "download"; entryId: string; quant: string } | null;
}

export interface AthanorError {
  code: string;
  message: string;
}

export function isAthanorError(e: unknown): e is AthanorError {
  return (
    typeof e === "object" &&
    e !== null &&
    "code" in e &&
    "message" in e &&
    typeof (e as AthanorError).message === "string"
  );
}
