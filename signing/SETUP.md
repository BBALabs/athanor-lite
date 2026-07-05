# Athanor Code Signing Setup

## One-time setup

```powershell
# Install the signing tools
winget install -e --id Microsoft.Azure.ArtifactSigningClientTools

# Make sure Azure CLI is logged in
az login
```

## Sign binaries

```powershell
# From the signing/ directory
.\sign-athanor.ps1

# Or sign a specific file
.\sign-athanor.ps1 -Target "C:\path\to\Athanor Lite_0.1.0_x64-setup.exe"
```

## Azure config

- Account: bba-athanor-signing (East US)
- Profile: athanor-public (PublicTrust)
- Subject: CN=Black Box Analytics LLC, O=Black Box Analytics LLC, L=Lansing, S=Michigan, C=US
- Identity validation expires: 10/7/2028
