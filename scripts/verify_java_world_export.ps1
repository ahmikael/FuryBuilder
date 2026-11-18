# Automated Minecraft Java server fixture for the verified world export (M8.12).
#
# This script never downloads, bundles or redistributes a server JAR. Point it at an
# official server JAR and a compatible Java executable that you obtained yourself.
# It requires an explicit -AcceptMinecraftEula switch: the script will not accept the
# Minecraft EULA on your behalf.
#
# Flow:
#   1. start the official server once on a fresh flat world, force-load chunk (0, 0),
#      verify the flat layers, then stop and keep the generated world as the fixture;
#   2. edit (0, -61, 0) from grass_block to gold_block through world_export_probe and
#      export a complete new world folder;
#   3. start the same official server on the exported folder, force-load the chunk and
#      verify the edited block and an untouched neighbor with /execute if block;
#   4. stop the server, decode the saved block states again with FuryBuilder, compare
#      source-tree hashes and write a JSON evidence file.
#
# Usage:
#   pwsh -File scripts/verify_java_world_export.ps1 `
#     -Java <java.exe> -ServerJar <server.jar> `
#     -ExpectedSha1 4707d00eb834b446575d89a61a11b5d548d8c001 `
#     -AcceptMinecraftEula
#
# Optional: -WorkRoot <empty-dir> -EvidencePath <json> -StartupTimeoutSeconds 180 -Keep

param(
    [Parameter(Mandatory = $true)][string]$Java,
    [Parameter(Mandatory = $true)][string]$ServerJar,
    [string]$ExpectedSha1 = '',
    [string]$WorkRoot = '',
    [string]$EvidencePath = '',
    [int]$StartupTimeoutSeconds = 180,
    [switch]$AcceptMinecraftEula,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'

if (-not $AcceptMinecraftEula) {
    throw 'The script never accepts the Minecraft EULA for you. Read https://www.minecraft.net/en-us/eula and pass -AcceptMinecraftEula only if you agree.'
}
if (-not (Test-Path -LiteralPath $Java)) { throw "Java executable not found: $Java" }
if (-not (Test-Path -LiteralPath $ServerJar)) { throw "server JAR not found: $ServerJar" }

$repoRoot = Split-Path -Parent $PSScriptRoot
$resolvedJava = (Resolve-Path -LiteralPath $Java).Path
$resolvedJar = (Resolve-Path -LiteralPath $ServerJar).Path
$jarSha1 = (Get-FileHash -Algorithm SHA1 -LiteralPath $resolvedJar).Hash.ToLowerInvariant()
if ($ExpectedSha1 -and $jarSha1 -ne $ExpectedSha1.ToLowerInvariant()) {
    throw "server JAR SHA-1 mismatch: expected $ExpectedSha1, got $jarSha1"
}
$javaVersion = (cmd /c "`"$resolvedJava`" -version 2>&1" | Out-String).Trim()

$stamp = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
if (-not $WorkRoot) {
    $WorkRoot = Join-Path $env:TEMP "fury-server-fixture-$stamp"
}
if (-not (Test-Path -LiteralPath $WorkRoot)) {
    New-Item -ItemType Directory -Path $WorkRoot | Out-Null
}
if (@(Get-ChildItem -LiteralPath $WorkRoot -Force).Count -ne 0) {
    throw "WorkRoot must be empty: $WorkRoot"
}
if (-not $EvidencePath) {
    $EvidencePath = Join-Path $repoRoot 'docs\testing\evidence\2026-11-18-server-fixture-1.21.4.json'
}

function Get-TreeHashes([string]$root) {
    $map = @{}
    $rootFull = [System.IO.Path]::GetFullPath($root).TrimEnd([char[]]@('\', '/'))
    Get-ChildItem -LiteralPath $root -Recurse -File -Force | ForEach-Object {
        $relative = $_.FullName.Substring($rootFull.Length).TrimStart([char[]]@('\', '/')).Replace('\', '/')
        $map[$relative] = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
    }
    return $map
}

function Compare-TreeHashes($before, $after) {
    $changed = @()
    foreach ($key in $before.Keys) {
        if (-not $after.ContainsKey($key) -or $after[$key] -ne $before[$key]) { $changed += $key }
    }
    foreach ($key in $after.Keys) {
        if (-not $before.ContainsKey($key)) { $changed += $key }
    }
    return $changed
}

function New-FreeLoopbackPort {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    $listener.Stop()
    return $port
}

function Write-ServerProperties([string]$root, [int]$port) {
    $lines = @(
        'allow-flight=false'
        'allow-nether=false'
        'difficulty=peaceful'
        'enable-command-block=false'
        'enable-query=false'
        'enable-rcon=false'
        'enforce-secure-profile=false'
        'force-gamemode=true'
        'gamemode=creative'
        'generate-structures=false'
        'generator-settings={"layers":[{"block":"minecraft:bedrock","height":1},{"block":"minecraft:dirt","height":2},{"block":"minecraft:grass_block","height":1}],"biome":"minecraft:plains"}'
        'level-name=world'
        'level-type=minecraft:flat'
        'max-players=1'
        'motd=FuryBuilder automated fixture'
        'online-mode=false'
        'pause-when-empty-seconds=0'
        'server-ip=127.0.0.1'
        "server-port=$port"
        'simulation-distance=2'
        'spawn-protection=0'
        'sync-chunk-writes=true'
        'view-distance=2'
    )
    [System.IO.File]::WriteAllLines(
        (Join-Path $root 'server.properties'),
        $lines,
        [System.Text.UTF8Encoding]::new($false)
    )
    [System.IO.File]::WriteAllText(
        (Join-Path $root 'eula.txt'),
        "eula=true`n",
        [System.Text.UTF8Encoding]::new($false)
    )
}

function Start-GameServer([string]$root, [string]$logPath) {
    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $resolvedJava
    $psi.Arguments = "-Xms512M -Xmx1G -jar `"$resolvedJar`" nogui"
    $psi.WorkingDirectory = $root
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $psi
    [void]$process.Start()
    $logStream = [System.IO.FileStream]::new(
        $logPath,
        [System.IO.FileMode]::Create,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::ReadWrite,
        1,
        [System.IO.FileOptions]::WriteThrough
    )
    $stdoutTask = $process.StandardOutput.BaseStream.CopyToAsync($logStream)
    $stderrTask = $process.StandardError.ReadToEndAsync()
    return [pscustomobject]@{
        Process     = $process
        LogPath     = $logPath
        GameLogPath = (Join-Path $root 'logs\latest.log')
        LogStream   = $logStream
        StdoutTask  = $stdoutTask
        StderrTask  = $stderrTask
    }
}

function Read-LogText([string]$path) {
    if (-not (Test-Path -LiteralPath $path)) {
        return ''
    }
    try {
        return ((Get-Content -LiteralPath $path -ErrorAction Stop) | Out-String)
    } catch {
        try {
            return ((Get-Content -LiteralPath $path -Tail 4000 -ErrorAction Stop) | Out-String)
        } catch {
            return ''
        }
    }
}

function Wait-ForLog($server, [string]$needle, [int]$timeoutSeconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($timeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $text = Read-LogText $server.GameLogPath
        if ($text.Contains($needle)) { return $true }
        if ($server.Process.HasExited) { break }
        Start-Sleep -Milliseconds 250
    }
    $tail = (Read-LogText $server.GameLogPath) -split "`n" | Select-Object -Last 40
    if ($tail.Count -eq 0) {
        $tail = (Read-LogText $server.LogPath) -split "`n" | Select-Object -Last 40
    }
    throw "server log did not contain '$needle' within $timeoutSeconds seconds.`n$($tail -join "`n")"
}

function Send-ServerCommand($server, [string]$command) {
    $server.Process.StandardInput.WriteLine($command)
    $server.Process.StandardInput.Flush()
}

function Stop-GameServer($server) {
    if (-not $server.Process.HasExited) {
        try {
            Send-ServerCommand $server 'stop'
            $server.Process.StandardInput.Close()
        } catch {
            # The process may already be gone; the wait below still reports the result.
        }
    }
    if (-not $server.Process.WaitForExit(120000)) {
        $server.Process.Kill()
        throw 'Minecraft server did not stop within 120 seconds'
    }
    $server.Process.WaitForExit()
    $null = $server.StdoutTask.Wait(15000)
    $stderr = $server.StderrTask.Result
    $server.LogStream.Flush()
    $server.LogStream.Dispose()
    return [pscustomobject]@{
        ExitCode   = $server.Process.ExitCode
        StdErr     = $stderr
        Log        = Read-LogText $server.GameLogPath
        StdoutLog  = Read-LogText $server.LogPath
    }
}

function Get-LogSummary([string]$log) {
    $lines = $log -split "`n"
    $versionLine = ($lines | Where-Object { $_ -match 'Starting minecraft server version' } | Select-Object -First 1)
    $doneLine = ($lines | Where-Object { $_ -match 'Done \(' } | Select-Object -First 1)
    $issuePatterns = @(
        'Failed to load chunk',
        'Failed to save chunk',
        'Chunk file at',
        'Failed to load world',
        'Exception in server tick',
        'Unknown or incomplete command',
        'That position is not loaded'
    )
    $issueLines = @()
    foreach ($line in $lines) {
        foreach ($pattern in $issuePatterns) {
            if ($line.Contains($pattern)) { $issueLines += $line.Trim(); break }
        }
    }
    $errorLines = @($lines | Where-Object { $_ -match 'ERROR\]' } | ForEach-Object { $_.Trim() })
    return [pscustomobject]@{
        VersionLine = if ($versionLine) { $versionLine.Trim() } else { $null }
        DoneLine    = if ($doneLine) { $doneLine.Trim() } else { $null }
        IssueLines  = $issueLines
        ErrorLines  = $errorLines
        Tail        = (($lines | Select-Object -Last 80) -join "`n")
    }
}

& cargo build -q -p furybuilder-world-io --examples
if ($LASTEXITCODE -ne 0) { throw 'cargo build for world-io examples failed' }

$generatorRoot = Join-Path $WorkRoot 'generator'
$verifierRoot = Join-Path $WorkRoot 'verifier'
$exportedWorld = Join-Path $WorkRoot 'exported-world'
$generatorLog = Join-Path $WorkRoot 'generator-stdout.log'
$verifierLog = Join-Path $WorkRoot 'verifier-stdout.log'
New-Item -ItemType Directory -Path $generatorRoot | Out-Null

Write-ServerProperties $generatorRoot (New-FreeLoopbackPort)
$generator = Start-GameServer $generatorRoot $generatorLog
$generatorResult = $null
try {
    $null = Wait-ForLog $generator 'Done (' $StartupTimeoutSeconds
    Send-ServerCommand $generator 'forceload add 0 0'
    Start-Sleep -Seconds 2
    Send-ServerCommand $generator 'execute if block 0 -61 0 minecraft:grass_block run say FURY_FIXTURE_GRASS_OK'
    Send-ServerCommand $generator 'execute if block 0 -62 0 minecraft:dirt run say FURY_FIXTURE_DIRT_OK'
    $null = Wait-ForLog $generator 'FURY_FIXTURE_GRASS_OK' 30
    $null = Wait-ForLog $generator 'FURY_FIXTURE_DIRT_OK' 30
} finally {
    $generatorResult = Stop-GameServer $generator
}
if ($generatorResult.ExitCode -ne 0) {
    throw "fixture generator exited with code $($generatorResult.ExitCode)"
}
$generatorSummary = Get-LogSummary $generatorResult.Log
if ($generatorSummary.VersionLine -notmatch '1\.21\.4') {
    throw "fixture generator is not Minecraft 1.21.4: $($generatorSummary.VersionLine)"
}
if ($generatorSummary.IssueLines.Count -gt 0) {
    throw "fixture generator logged chunk/command issues: $($generatorSummary.IssueLines -join ' | ')"
}

$fixtureWorld = Join-Path $generatorRoot 'world'
if (-not (Test-Path -LiteralPath $fixtureWorld)) { throw "fixture world was not generated: $fixtureWorld" }
$fixtureBefore = Get-TreeHashes $fixtureWorld

$probeOutput = & cargo run -q -p furybuilder-world-io --example world_export_probe -- `
    $fixtureWorld $exportedWorld minecraft:overworld 0 -61 0 minecraft:gold_block --rebuild-derived 2>&1 | Out-String
if ($LASTEXITCODE -ne 0) { throw "world_export_probe failed ($LASTEXITCODE):`n$probeOutput" }
$probe = $probeOutput | ConvertFrom-Json
if (-not $probe.report.sourceUnchanged) { throw 'export reported a changed source world' }
if (-not $probe.report.outputRevalidated) { throw 'export did not revalidate its output' }
if (-not $probe.report.copiedFilesMatchSource) { throw 'export did not verify copied files' }
if ($probe.report.replacedChunks -ne 1) { throw "export replaced $($probe.report.replacedChunks) chunks, expected 1" }
if ($probe.verified[0].state -ne 'minecraft:gold_block') {
    throw "export read-back was $($probe.verified[0].state), expected minecraft:gold_block"
}

$fixtureAfterExport = Get-TreeHashes $fixtureWorld
$fixtureChangedAfterExport = Compare-TreeHashes $fixtureBefore $fixtureAfterExport
if ($fixtureChangedAfterExport.Count -ne 0) {
    throw "fixture source changed during export: $($fixtureChangedAfterExport -join ', ')"
}

New-Item -ItemType Directory -Path $verifierRoot | Out-Null
Move-Item -LiteralPath $exportedWorld -Destination (Join-Path $verifierRoot 'world')
$verifierWorld = Join-Path $verifierRoot 'world'
$verifierBefore = Get-TreeHashes $verifierWorld

Write-ServerProperties $verifierRoot (New-FreeLoopbackPort)
$verifier = Start-GameServer $verifierRoot $verifierLog
$verifierResult = $null
try {
    $null = Wait-ForLog $verifier 'Done (' $StartupTimeoutSeconds
    Send-ServerCommand $verifier 'forceload add 0 0'
    Start-Sleep -Seconds 2
    Send-ServerCommand $verifier 'execute if block 0 -61 0 minecraft:gold_block run say FURY_EXPORT_EDIT_OK'
    Send-ServerCommand $verifier 'execute if block 0 -61 0 minecraft:grass_block run say FURY_EXPORT_STALE_GRASS'
    Send-ServerCommand $verifier 'execute if block 0 -62 0 minecraft:dirt run say FURY_EXPORT_NEIGHBOR_OK'
    $null = Wait-ForLog $verifier 'FURY_EXPORT_EDIT_OK' 30
    $null = Wait-ForLog $verifier 'FURY_EXPORT_NEIGHBOR_OK' 30
} finally {
    $verifierResult = Stop-GameServer $verifier
}
if ($verifierResult.ExitCode -ne 0) {
    throw "verifier server exited with code $($verifierResult.ExitCode)"
}
$verifierSummary = Get-LogSummary $verifierResult.Log
if ($verifierSummary.IssueLines.Count -gt 0) {
    throw "verifier server logged chunk/command issues: $($verifierSummary.IssueLines -join ' | ')"
}
if ($verifierResult.Log.Contains('FURY_EXPORT_STALE_GRASS')) {
    throw 'the verifier found grass_block at the edited position'
}

$verifierAfter = Get-TreeHashes $verifierWorld
$verifierChanged = Compare-TreeHashes $verifierBefore $verifierAfter

$postSaveOutput = & cargo run -q -p furybuilder-world-io --example derived_probe -- `
    $verifierWorld --blocks 0 -61 0 0 -62 0 2>&1 | Out-String
if ($LASTEXITCODE -ne 0) { throw "derived_probe failed ($LASTEXITCODE):`n$postSaveOutput" }
$postSave = $postSaveOutput | ConvertFrom-Json
$editedAfter = @($postSave.blocks | Where-Object { $_.x -eq 0 -and $_.y -eq -61 -and $_.z -eq 0 })[0]
$neighborAfter = @($postSave.blocks | Where-Object { $_.x -eq 0 -and $_.y -eq -62 -and $_.z -eq 0 })[0]
if ($editedAfter.state -ne 'minecraft:gold_block') {
    throw "post-save block at (0,-61,0) is $($editedAfter.state), expected minecraft:gold_block"
}
if ($neighborAfter.state -ne 'minecraft:dirt') {
    throw "post-save block at (0,-62,0) is $($neighborAfter.state), expected minecraft:dirt"
}

$fixtureFinal = Get-TreeHashes $fixtureWorld
$fixtureChangedFinal = Compare-TreeHashes $fixtureBefore $fixtureFinal
if ($fixtureChangedFinal.Count -ne 0) {
    throw "fixture source changed during the run: $($fixtureChangedFinal -join ', ')"
}
if ($verifierChanged.Count -eq 0) {
    throw 'the verifier server did not rewrite any file; the save/load proof is inconclusive'
}

$evidence = [ordered]@{
    tool = [ordered]@{ name = 'furybuilder-server-fixture'; version = 1 }
    experiment = 'official Minecraft Java 1.21.4 server generates a flat fixture, FuryBuilder exports an edited complete world folder, the same server loads it and verifies block states'
    generatedAt = [DateTimeOffset]::UtcNow.ToString('o')
    java = [ordered]@{ executable = $resolvedJava; version = $javaVersion }
    serverJar = [ordered]@{ path = $resolvedJar; sha1 = $jarSha1; version = '1.21.4' }
    eula = 'explicit -AcceptMinecraftEula switch was required; the script does not accept the EULA automatically'
    workRoot = $WorkRoot
    artifactsRetained = [bool]$Keep
    fixture = [ordered]@{
        path = $fixtureWorld
        files = $fixtureBefore.Count
        bytes = (($fixtureBefore.Keys | ForEach-Object { (Get-Item -LiteralPath (Join-Path $fixtureWorld $_)).Length } | Measure-Object -Sum).Sum)
        generatorExitCode = $generatorResult.ExitCode
        generatorVersionLine = $generatorSummary.VersionLine
        generatorDoneLine = $generatorSummary.DoneLine
        generatorIssueLines = $generatorSummary.IssueLines
        generatorErrorLines = $generatorSummary.ErrorLines
        generatorLogTail = $generatorSummary.Tail
        sourceUnchangedAfterExport = ($fixtureChangedAfterExport.Count -eq 0)
        sourceUnchangedAfterVerification = ($fixtureChangedFinal.Count -eq 0)
    }
    export = [ordered]@{
        destination = $verifierWorld
        report = $probe.report
        verifiedBlocks = $probe.verified
        changes = $probe.changes
    }
    verifier = [ordered]@{
        exitCode = $verifierResult.ExitCode
        versionLine = $verifierSummary.VersionLine
        doneLine = $verifierSummary.DoneLine
        issueLines = $verifierSummary.IssueLines
        errorLines = $verifierSummary.ErrorLines
        logTail = $verifierSummary.Tail
        editMarkerFound = $verifierResult.Log.Contains('FURY_EXPORT_EDIT_OK')
        staleGrassMarkerAbsent = -not $verifierResult.Log.Contains('FURY_EXPORT_STALE_GRASS')
        neighborMarkerFound = $verifierResult.Log.Contains('FURY_EXPORT_NEIGHBOR_OK')
        filesChangedByServer = $verifierChanged.Count
        changedFiles = $verifierChanged
    }
    postSaveDecodedBlocks = $postSave.blocks
    conclusions = [ordered]@{
        serverVersionIsTarget = $generatorSummary.VersionLine -match '1\.21\.4'
        fixtureGeneratedByOfficialServer = (Test-Path -LiteralPath $fixtureWorld)
        exportSourceUnchanged = ($fixtureChangedAfterExport.Count -eq 0 -and $fixtureChangedFinal.Count -eq 0)
        exportRevalidated = $probe.report.outputRevalidated
        exportedChunksMatchSourceCount = $probe.report.chunks
        verifierLoadedExportedWorld = ($null -ne $verifierSummary.DoneLine)
        editedBlockVisibleInServer = $verifierResult.Log.Contains('FURY_EXPORT_EDIT_OK')
        staleGrassAbsent = -not $verifierResult.Log.Contains('FURY_EXPORT_STALE_GRASS')
        untouchedNeighborIntact = $verifierResult.Log.Contains('FURY_EXPORT_NEIGHBOR_OK')
        editedBlockPersistedAfterSave = ($editedAfter.state -eq 'minecraft:gold_block')
        serverRewroteSaveFiles = ($verifierChanged.Count -gt 0)
        noChunkOrCommandIssues = ($generatorSummary.IssueLines.Count -eq 0 -and $verifierSummary.IssueLines.Count -eq 0)
    }
    reproduce = 'pwsh -File scripts/verify_java_world_export.ps1 -Java <java.exe> -ServerJar <server.jar> -ExpectedSha1 4707d00eb834b446575d89a61a11b5d548d8c001 -AcceptMinecraftEula'
    limitations = @(
        'one official 1.21.4 server, one generated flat fixture and one edited block',
        'the client was not launched; a licensed Minecraft client installation was not available in this environment',
        'the fixture has no block entities, entities, scheduled ticks or non-empty POI records',
        'the server JAR and Java runtime are user-provided and are not distributed by FuryBuilder',
        'the work root is deleted after a successful run unless -Keep is passed; hashes and server log excerpts are recorded here'
    )
}

$evidenceFullPath = if ([System.IO.Path]::IsPathRooted($EvidencePath)) { $EvidencePath } else { Join-Path $repoRoot $EvidencePath }
[System.IO.File]::WriteAllText(
    $evidenceFullPath,
    ($evidence | ConvertTo-Json -Depth 24),
    [System.Text.UTF8Encoding]::new($false)
)
& (Join-Path $repoRoot 'apps\desktop\node_modules\.bin\prettier.cmd') --write $evidenceFullPath | Out-Host

"WORK_ROOT=$WorkRoot"
"FIXTURE_FILES=$($fixtureBefore.Count) EXPORT_CHUNKS=$($probe.report.chunks) REPLACED_CHUNKS=$($probe.report.replacedChunks)"
"GENERATOR_EXIT=$($generatorResult.ExitCode) VERIFIER_EXIT=$($verifierResult.ExitCode)"
"EDIT_MARKER=$($verifierResult.Log.Contains('FURY_EXPORT_EDIT_OK')) STALE_GRASS=$($verifierResult.Log.Contains('FURY_EXPORT_STALE_GRASS')) NEIGHBOR=$($verifierResult.Log.Contains('FURY_EXPORT_NEIGHBOR_OK'))"
"POST_SAVE_EDIT=$($editedAfter.state) NEIGHBOR=$($neighborAfter.state)"
"VERIFIER_CHANGED_FILES=$($verifierChanged.Count)"
"SOURCE_UNCHANGED=$($fixtureChangedFinal.Count -eq 0)"
"EVIDENCE=$evidenceFullPath"

if (-not $Keep) {
    Remove-Item -LiteralPath $WorkRoot -Recurse -Force
}
