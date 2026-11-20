#requires -Version 5.1
<#
.SYNOPSIS
    Verifies the provenance of built Windows release artifacts.

.DESCRIPTION
    Records a SHA-256 digest, file version and Authenticode signature status for every
    supplied artifact, then applies a release policy:

    - With -RequireSigned (the policy for signed public releases) every artifact must have
      a valid Authenticode signature. Missing, expired or tampered signatures fail the run.
    - Without -RequireSigned (the honest beta state while no certificate exists) unsigned
      artifacts pass but are recorded as unsigned and reported as a limitation. A signature
      that exists but is invalid always fails, even in this mode.

    Evidence is written as JSON with no private key material and no absolute user paths.

.PARAMETER Artifact
    One or more paths to installers or executables to verify.

.PARAMETER ExpectedVersion
    Optional version that each executable's ProductVersion must match.

.PARAMETER RequireSigned
    Fail unless every artifact has a valid Authenticode signature.

.PARAMETER OutFile
    Evidence JSON path.

.EXAMPLE
    pwsh -File scripts/verify-release-artifacts.ps1 `
        -Artifact target/release/bundle/nsis/FuryBuilder_0.1.0_x64-setup.exe `
        -ExpectedVersion 0.1.0
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string[]]$Artifact,
    [string]$ExpectedVersion,
    [switch]$RequireSigned,
    [string]$OutFile = "artifacts/release-security/signature-evidence.json"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version 2.0

function Get-Sha256 {
    param([string]$Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-ArtifactRecord {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Artifact not found: $Path"
    }
    $item = Get-Item -LiteralPath $Path
    $signature = Get-AuthenticodeSignature -LiteralPath $item.FullName
    $signer = $null
    if ($null -ne $signature.SignerCertificate) {
        $signer = $signature.SignerCertificate.Subject
    }
    $version = $item.VersionInfo.ProductVersion
    [pscustomobject]@{
        file            = $item.Name
        bytes           = $item.Length
        sha256          = Get-Sha256 -Path $item.FullName
        productVersion  = $version
        signatureStatus = [string]$signature.Status
        signerSubject   = $signer
        isSigned        = [string]$signature.Status -eq "Valid"
    }
}

$artifacts = @()
foreach ($entry in $Artifact) {
    foreach ($part in $entry.Split(",")) {
        $trimmed = $part.Trim()
        if (-not [string]::IsNullOrWhiteSpace($trimmed)) {
            $artifacts += $trimmed
        }
    }
}

$records = New-Object System.Collections.Generic.List[object]
foreach ($path in $artifacts) {
    $records.Add((Get-ArtifactRecord -Path $path)) | Out-Null
}

$problems = New-Object System.Collections.Generic.List[string]
foreach ($record in $records) {
    if ($record.signatureStatus -notin @("Valid", "NotSigned")) {
        $problems.Add("$($record.file): Authenticode status $($record.signatureStatus) (tampered or untrusted signature)")
    }
    if (-not [string]::IsNullOrWhiteSpace($ExpectedVersion) -and $record.productVersion -ne $ExpectedVersion) {
        $problems.Add("$($record.file): ProductVersion $($record.productVersion) does not match expected $ExpectedVersion")
    }
}
if ($RequireSigned) {
    foreach ($record in $records) {
        if (-not $record.isSigned) {
            $problems.Add("$($record.file): unsigned, but -RequireSigned was requested")
        }
    }
}

$unsigned = @($records | Where-Object { -not $_.isSigned } | ForEach-Object { $_.file })
$limitations = @()
if ($unsigned.Count -gt 0) {
    $limitations += "Unsigned artifacts: $($unsigned -join ', '). Windows SmartScreen may warn until a code-signing certificate is configured."
}

$passed = $problems.Count -eq 0
$result = [ordered]@{
    schemaVersion    = 1
    milestone        = "M11"
    task             = "release-security-signature"
    recordedAt       = (Get-Date).ToString("o")
    requireSigned    = [bool]$RequireSigned
    expectedVersion  = $ExpectedVersion
    artifacts        = $records.ToArray()
    limitations      = $limitations
    pass             = $passed
    problems         = $problems.ToArray()
}

$directory = Split-Path -Parent $OutFile
if (-not [string]::IsNullOrWhiteSpace($directory)) {
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
}
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText([System.IO.Path]::GetFullPath($OutFile), ($result | ConvertTo-Json -Depth 10), $utf8NoBom)

foreach ($record in $records) {
    Write-Host ("{0}: {1} [{2}] sha256={3}" -f $record.file, $record.signatureStatus, $record.productVersion, $record.sha256)
}
foreach ($problem in $problems) {
    Write-Host ("PROBLEM: " + $problem)
}
Write-Host ("Evidence: " + $OutFile)
if (-not $passed) { exit 1 }
exit 0
