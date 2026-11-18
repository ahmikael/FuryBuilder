# M9.1 evidence: local Minecraft Java asset discovery.
#
# Reads the user's existing launcher root and client JAR only. Nothing is downloaded,
# extracted or modified. Synthetic resource packs are created in the temporary work dir
# to exercise pack.mcmeta validation; they are not distributed.
#
# Usage:
#   pwsh -File scripts/probe_minecraft_assets.ps1 `
#     -MinecraftRoot D:\mitrium-launcher\data `
#     [-EvidencePath docs/testing/evidence/2026-11-18-minecraft-assets-1.21.4.json]

param(
    [Parameter(Mandatory = $true)][string]$MinecraftRoot,
    [string]$EvidencePath = ''
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path -LiteralPath $MinecraftRoot)) {
    throw "Minecraft root not found: $MinecraftRoot"
}
$resolvedRoot = (Resolve-Path -LiteralPath $MinecraftRoot).Path
$clientJar = Join-Path $resolvedRoot 'versions\1.21.4\1.21.4.jar'
if (-not (Test-Path -LiteralPath $clientJar)) {
    throw "Minecraft 1.21.4 client JAR not found: $clientJar"
}
if (-not $EvidencePath) {
    $EvidencePath = Join-Path $repoRoot 'docs\testing\evidence\2026-11-18-minecraft-assets-1.21.4.json'
}

$stamp = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$workRoot = Join-Path $env:TEMP "fury-minecraft-assets-$stamp"
New-Item -ItemType Directory -Path $workRoot | Out-Null

function Invoke-AssetProbe([string[]]$arguments) {
    $output = & cargo run -q -p furybuilder-asset-source --example asset_source_probe -- @arguments 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) {
        throw "asset probe failed ($LASTEXITCODE):`n$output"
    }
    return $output | ConvertFrom-Json
}

function Get-FileSnapshot([string[]]$paths) {
    $items = @()
    foreach ($path in $paths) {
        $item = Get-Item -LiteralPath $path
        $items += [ordered]@{
            path = $path
            bytes = $item.Length
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
        }
    }
    return $items
}

function New-ZipPack([string]$path, [hashtable]$entries) {
    $zip = [System.IO.Compression.ZipFile]::Open($path, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($name in $entries.Keys) {
            $entry = $zip.CreateEntry($name, [System.IO.Compression.CompressionLevel]::Optimal)
            $stream = $entry.Open()
            $writer = [System.IO.StreamWriter]::new($stream, [System.Text.UTF8Encoding]::new($false))
            $writer.Write($entries[$name])
            $writer.Dispose()
            $stream.Dispose()
        }
    } finally {
        $zip.Dispose()
    }
}

$watchedFiles = @(
    $clientJar,
    (Join-Path $resolvedRoot 'versions\1.21.4\1.21.4.json'),
    (Join-Path $resolvedRoot 'assets\indexes\19.json')
)
$before = Get-FileSnapshot $watchedFiles

$defaultDiscovery = Invoke-AssetProbe @('discover')
$installation = Invoke-AssetProbe @('inspect', $resolvedRoot)
$client = Invoke-AssetProbe @('inspect', $clientJar)

$validPack = Join-Path $workRoot 'valid-pack.zip'
New-ZipPack $validPack ([ordered]@{
    'pack.mcmeta' = '{"pack":{"pack_format":46,"description":"M9.1 synthetic valid pack"}}'
    'assets/minecraft/blockstates/stone.json' = '{}'
    'assets/minecraft/textures/block/stone.png' = 'synthetic'
})
$wrongPack = Join-Path $workRoot 'wrong-format.zip'
New-ZipPack $wrongPack ([ordered]@{
    'pack.mcmeta' = '{"pack":{"pack_format":15,"description":"M9.1 synthetic old pack"}}'
})
$missingMetaFolder = Join-Path $workRoot 'missing-metadata'
New-Item -ItemType Directory -Path (Join-Path $missingMetaFolder 'assets\minecraft\blockstates') -Force | Out-Null

$validPackReport = Invoke-AssetProbe @('inspect', $validPack)
$wrongPackReport = Invoke-AssetProbe @('inspect', $wrongPack)
$missingMetaReport = Invoke-AssetProbe @('inspect', $missingMetaFolder)

$after = Get-FileSnapshot $watchedFiles
$unchanged = $true
for ($index = 0; $index -lt $before.Count; $index++) {
    if ($before[$index].sha256 -ne $after[$index].sha256 -or $before[$index].bytes -ne $after[$index].bytes) {
        $unchanged = $false
    }
}

$targetVersion = $installation.inspection.version
$evidence = [ordered]@{
    tool = [ordered]@{ name = 'furybuilder-minecraft-assets'; version = 1 }
    generatedAt = [DateTimeOffset]::UtcNow.ToString('o')
    experiment = 'discover the conventional .minecraft root, inspect an explicitly selected launcher root and client JAR, and validate synthetic resource packs read-only'
    target = [ordered]@{
        minecraftVersion = '1.21.4'
        dataVersion = 4189
        resourcePackFormat = 46
    }
    roots = [ordered]@{
        selected = $resolvedRoot
        defaultRoots = $defaultDiscovery.discovery.searchedRoots
        defaultRootPresent = $defaultDiscovery.discovery.defaultRootPresent
        defaultProblems = $defaultDiscovery.discovery.problems
        manualHint = $defaultDiscovery.discovery.manualHint
    }
    installation = [ordered]@{
        kind = $installation.inspection.kind
        valid = $installation.inspection.valid
        targetMatch = $installation.inspection.targetMatch
        versions = $installation.inspection.installation.versions.Count
        compatibleVersions = @($installation.inspection.installation.compatibleVersions)
        assetIndexes = $installation.inspection.installation.assetIndexes
        resourcePacks = $installation.inspection.installation.resourcePacks.Count
        warnings = @($installation.inspection.installation.warnings)
        targetVersion = $targetVersion
        errors = @($installation.inspection.errors)
    }
    clientJar = [ordered]@{
        kind = $client.inspection.kind
        valid = $client.inspection.valid
        errors = @($client.inspection.errors)
        version = $client.inspection.version
    }
    syntheticResourcePacks = [ordered]@{
        note = 'synthetic packs were generated only in the temporary work directory'
        valid = $validPackReport.inspection
        wrongFormat = $wrongPackReport.inspection
        missingMetadata = $missingMetaReport.inspection
    }
    sourceFiles = [ordered]@{
        before = $before
        after = $after
        unchanged = $unchanged
    }
    conclusions = [ordered]@{
        defaultRootAbsentReported = (-not $defaultDiscovery.discovery.defaultRootPresent)
        manualRootAccepted = $installation.inspection.valid
        targetVersionCompatible = ($targetVersion.status -eq 'compatible')
        clientJarSha1Verified = $targetVersion.checksumVerified
        clientJarWorldVersion4189 = ($targetVersion.worldVersion -eq 4189)
        clientJarPackFormat46 = ($targetVersion.resourcePackFormat -eq 46)
        assetIndexVerified = $targetVersion.assetIndexChecksumVerified
        manualClientJarAccepted = $client.inspection.valid
        validResourcePackAccepted = $validPackReport.inspection.valid
        wrongFormatRejected = (-not $wrongPackReport.inspection.valid)
        missingMetadataRejected = (-not $missingMetaReport.inspection.valid)
        sourceFilesUnchanged = $unchanged
        moddedProfileCount = @($installation.inspection.installation.versions | Where-Object { $_.status -eq 'modded' }).Count
        compatibleVersionCount = @($installation.inspection.installation.compatibleVersions).Count
    }
    sourceFileCount = $watchedFiles.Count
    reproduce = 'pwsh -File scripts/probe_minecraft_assets.ps1 -MinecraftRoot <root>'
    limitations = @(
        'the selected root is a third-party launcher layout under D:\mitrium-launcher, inspected read-only as a manual path',
        'no vanilla .minecraft folder existed in the default APPDATA location on this machine',
        'synthetic resource packs validate parsing and format checks; no real resource pack was available',
        'the client JAR is never launched in this task; renderer integration is M9.2',
        'Mojang JARs and assets remain outside the FuryBuilder repository and distribution'
    )
}

$evidenceFullPath = if ([System.IO.Path]::IsPathRooted($EvidencePath)) { $EvidencePath } else { Join-Path $repoRoot $EvidencePath }
[System.IO.File]::WriteAllText(
    $evidenceFullPath,
    ($evidence | ConvertTo-Json -Depth 30),
    [System.Text.UTF8Encoding]::new($false)
)
& (Join-Path $repoRoot 'apps\desktop\node_modules\.bin\prettier.cmd') --write $evidenceFullPath | Out-Host

"WORK_ROOT=$workRoot"
"ROOT_VALID=$($installation.inspection.valid) TARGET=$($targetVersion.id) STATUS=$($targetVersion.status) SHA1_OK=$($targetVersion.checksumVerified) DV=$($targetVersion.worldVersion) PACK=$($targetVersion.resourcePackFormat)"
"CLIENT_VALID=$($client.inspection.valid) ASSET_INDEX_OK=$($targetVersion.assetIndexChecksumVerified)"
"PACK_VALID=$($validPackReport.inspection.valid) PACK_WRONG=$($wrongPackReport.inspection.valid) PACK_MISSING=$($missingMetaReport.inspection.valid)"
"SOURCE_UNCHANGED=$unchanged"
"EVIDENCE=$evidenceFullPath"

Remove-Item -LiteralPath $workRoot -Recurse -Force
