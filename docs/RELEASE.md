# Athanor — Release Runbook

The two prerequisites only Tony can complete are marked **[TONY]**.

## 1. Code signing (blocks public Windows distribution)

Unsigned installers hit SmartScreen ("Windows protected your PC") — fatal for
the novice audience. Options, best first:

- **[TONY] Azure Trusted Signing** (recommended): ~$9.99/mo, no hardware token,
  reputation accrues to the Azure account, integrates with `signtool`. Requires
  an Azure account + identity validation for Black Box Analytics.
- **[TONY] OV code-signing certificate** (~$80–200/yr via Certum/SSL.com):
  ships on a hardware token since 2023; SmartScreen reputation builds slowly.
- EV certificate: instant SmartScreen reputation, ~$300+/yr, hardware token.

Wire-up once obtained (`src-tauri/tauri.conf.json` → `bundle > windows`):

```jsonc
"windows": {
  // Azure Trusted Signing / custom tooling:
  "signCommand": "trusted-signing-cli sign %1",
  // OR classic certificate:
  "certificateThumbprint": "<thumbprint>",
  "digestAlgorithm": "sha256",
  "timestampUrl": "http://timestamp.digicert.com"
}
```

## 2. Auto-updater (wired; needs a live endpoint)

Already in place:
- `tauri-plugin-updater` registered; `bundle.createUpdaterArtifacts: true`.
- Update-artifact keypair generated: private key at
  `%USERPROFILE%\.athanor-release\updater.key` (**never commit; back it up —
  losing it orphans every installed copy**). Public key is embedded in
  `tauri.conf.json > plugins.updater.pubkey`.
- Endpoint configured: `https://releases.bbasecure.com/athanor/latest.json`.
- In-app "Check for updates" (Settings) — reports honestly while the endpoint
  is not live.

**[TONY]** Stand up the endpoint (static file on bbasecure.com is enough):

```json
{
  "version": "0.2.0",
  "notes": "What changed.",
  "pub_date": "2026-07-10T00:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<contents of the generated .sig file>",
      "url": "https://releases.bbasecure.com/athanor/Athanor_0.2.0_x64-setup.exe"
    }
  }
}
```

## 3. Building a release

**Build prerequisite (RAG/LanceDB):** the Rust build needs `protoc` (protobuf
compiler) available at build time — LanceDB's `lance-*` crates compile protobuf
definitions. It is **build-time only**; the shipped binary embeds LanceDB with
no runtime dependency. Install once (any of: `winget install protobuf`, the
prebuilt binary from github.com/protocolbuffers/protobuf/releases, or a package
manager) and set `PROTOC` to its path, e.g.
`$env:PROTOC = "$env:USERPROFILE\.local\protoc\bin\protoc.exe"`.

```powershell
$env:PROTOC = "<path-to-protoc.exe>"
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$env:USERPROFILE\.athanor-release\updater.key"
npm run tauri build          # produces installer + .sig updater artifacts
```

Pre-release gate (all must pass):

```powershell
cd src-tauri; cargo test; cargo clippy --all-targets   # zero warnings
cd ..; npm run build                                   # tsc clean
$env:ATHANOR_SELFTEST="chat";   npm run tauri dev      # SELFTEST PASS
$env:ATHANOR_SELFTEST="import"; npm run tauri dev      # SELFTEST PASS
$env:ATHANOR_SELFTEST="rag";    npm run tauri dev      # index→embed→retrieve→cited answer PASS
$env:ATHANOR_SELFTEST="mcp";    npm run tauri dev      # connect server-everything, echo tool PASS
# Orphan guard: ATHANOR_SELFTEST=serve, then `taskkill /F /IM athanor.exe`,
# then verify: Get-Process llama-server → must be empty.
```

Upload the installer + write `latest.json` with the `.sig` contents. Done.

## 4. Athanor Lite — the free "what can my machine run" edition

Lite is a **build-time cut of the same app**: hardware scan → ranked
recommendations → one-click download-verify-chat, plus Ollama adopt-in-place.
Workspaces, RAG, MCP, Tune, and Compare stay in the binary's backend but are
not mounted in the UI. Same identifier and data root as the full app, so a
Lite install upgraded to full Athanor keeps every model and conversation.

The edition gate is a single Vite mode flag (`src/lib/edition.ts`); the Rust
side is identical in both editions.

```powershell
# Dev (real hardware):
npm run tauri:lite:dev

# Dev (browser design harness; add #cpu or #dualgpu to the URL to preview
# CPU-only and pooled multi-GPU machines):
npm run dev:lite

# Release build — same signing/updater env as §3:
npm run tauri:lite:build
```

Lite pre-release gate: the §3 gate plus
`$env:ATHANOR_SELFTEST="chat"; npm run tauri:lite:dev` (exercises the exact
engine-launch path Lite's one-click flow uses), and a manual pass of the
first-run flow: fresh data root → scan → download the recommended pick →
auto-land in chat → first reply.
