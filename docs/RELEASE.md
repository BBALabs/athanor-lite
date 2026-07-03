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

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$env:USERPROFILE\.athanor-release\updater.key"
npm run tauri build          # produces installer + .sig updater artifacts
```

Pre-release gate (all must pass):

```powershell
cd src-tauri; cargo test; cargo clippy --all-targets   # zero warnings
cd ..; npm run build                                   # tsc clean
$env:ATHANOR_SELFTEST="chat";   npm run tauri dev      # SELFTEST PASS
$env:ATHANOR_SELFTEST="import"; npm run tauri dev      # SELFTEST PASS
# Orphan guard: ATHANOR_SELFTEST=serve, then `taskkill /F /IM athanor.exe`,
# then verify: Get-Process llama-server → must be empty.
```

Upload the installer + write `latest.json` with the `.sig` contents. Done.
