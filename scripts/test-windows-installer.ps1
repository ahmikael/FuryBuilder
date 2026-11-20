#requires -Version 5.1
<#
.SYNOPSIS
    Verifies the Windows installers produced by `pnpm tauri:build`.

.DESCRIPTION
    Exercises the NSIS installer end to end: clean install, application startup,
    upgrade over an existing install, and uninstall. The optional MSI path
    reproduces the historical 2026-11-19 evidence; current releases build only
    the NSIS package.

    The real application data directory is backed up before the test and restored
    afterwards. A marker file created inside it proves that an upgrade keeps
    user data.

.PARAMETER OldInstaller
    NSIS installer for the version that is installed first.

.PARAMETER NewInstaller
    Optional NSIS installer for the upgrade. When omitted, the upgrade scenario is
    recorded as skipped.

.PARAMETER MsiInstaller
    Optional legacy MSI package to validate by administrative extraction.
    Retained to reproduce the 2026-11-19 evidence; it is not part of current releases.

.PARAMETER OutDir
    Evidence directory. Receives installer-evidence.json and installer-test.log.

.EXAMPLE
    pwsh -File scripts/test-windows-installer.ps1 `
        -OldInstaller target/release/bundle/nsis/FuryBuilder_0.1.0_x64-setup.exe `
        -NewInstaller target/release/bundle/nsis/FuryBuilder_0.1.1_x64-setup.exe
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$OldInstaller,
    [string]$NewInstaller,
    [string]$MsiInstaller,
    [string]$OutDir = "artifacts/windows-installer"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version 2.0

trap {
    Write-Host ("TRAP line " + $_.InvocationInfo.ScriptLineNumber + ": " + $_.Exception.GetType().FullName + " - " + $_.Exception.Message)
    Write-Host ($_.ScriptStackTrace)
    exit 2
}

$script:lines = New-Object System.Collections.Generic.List[string]
$script:scenarios = New-Object System.Collections.Generic.List[object]
$script:installers = New-Object System.Collections.Generic.List[object]
$script:limitations = New-Object System.Collections.Generic.List[string]

function Write-Log {
    param([string]$Message)
    $line = "[{0}] {1}" -f (Get-Date).ToString("yyyy-MM-dd HH:mm:ss"), $Message
    $script:lines.Add($line) | Out-Null
    Write-Host $Message
}

function Resolve-ExistingPath {
    param([string]$Path, [string]$Label)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label not found: $Path"
    }
    (Resolve-Path -LiteralPath $Path).Path
}

function Get-Sha256 {
    param([string]$Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-FuryUninstallEntry {
    $roots = @(
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        foreach ($key in Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue) {
            $props = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction SilentlyContinue
            if ($null -ne $props -and $props.PSObject.Properties.Name -contains "DisplayName" -and $props.DisplayName -eq "FuryBuilder") {
                $location = $props.InstallLocation
                if ($null -ne $location) { $location = $location.Trim('"') }
                $uninstallString = $props.UninstallString
                if ($null -ne $uninstallString) { $uninstallString = $uninstallString.Trim('"') }
                return [pscustomobject]@{
                    key             = $key.PSPath
                    displayName     = $props.DisplayName
                    displayVersion  = $props.DisplayVersion
                    installLocation = $location
                    uninstallString = $uninstallString
                    publisher       = $props.Publisher
                }
            }
        }
    }
    return $null
}

function Wait-ForUninstallEntry {
    param([int]$TimeoutSeconds = 60)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $entry = Get-FuryUninstallEntry
        if ($null -ne $entry) { return $entry }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "FuryBuilder uninstall registry entry did not appear within $TimeoutSeconds seconds"
}

function Wait-ForPathRemoval {
    param([string]$Path, [int]$TimeoutSeconds = 90)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        if (-not (Test-Path -LiteralPath $Path)) { return $true }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    return (-not (Test-Path -LiteralPath $Path))
}

function Wait-ForUninstallEntryRemoval {
    param([int]$TimeoutSeconds = 90)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        if ($null -eq (Get-FuryUninstallEntry)) { return $true }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    return ($null -eq (Get-FuryUninstallEntry))
}

function Get-InstalledExecutable {
    param([string]$InstallLocation)
    foreach ($name in @("FuryBuilder.exe", "furybuilder-desktop.exe")) {
        $candidate = Join-Path $InstallLocation $name
        if (Test-Path -LiteralPath $candidate) { return $candidate }
    }
    return $null
}

function Get-ExeProductVersion {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    (Get-Item -LiteralPath $Path).VersionInfo.ProductVersion
}

function Get-WebView2Version {
    $roots = @(
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients",
        "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients",
        "HKCU:\Software\Microsoft\EdgeUpdate\Clients"
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        foreach ($key in Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue) {
            $props = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction SilentlyContinue
            if ($null -eq $props) { continue }
            if ($props.PSObject.Properties.Name -contains "name" -and $props.PSObject.Properties.Name -contains "pv") {
                if ($props.name -like "*WebView2*" -and -not [string]::IsNullOrWhiteSpace($props.pv)) {
                    return $props.pv
                }
            }
        }
    }
    return $null
}

function Get-AppDataPath {
    Join-Path $env:APPDATA "com.furybuilder.desktop"
}

function Backup-AppData {
    $source = Get-AppDataPath
    if (-not (Test-Path -LiteralPath $source)) {
        return [pscustomobject]@{ Existed = $false; Backup = $null }
    }
    $backup = Join-Path $env:TEMP ("furybuilder-installer-appdata-" + [guid]::NewGuid().ToString("N"))
    Copy-Item -LiteralPath $source -Destination $backup -Recurse -Force
    [pscustomobject]@{ Existed = $true; Backup = $backup }
}

function Restore-AppData {
    param($Snapshot)
    $target = Get-AppDataPath
    if (Test-Path -LiteralPath $target) {
        Remove-Item -LiteralPath $target -Recurse -Force
    }
    if ($Snapshot.Existed) {
        Copy-Item -LiteralPath $Snapshot.Backup -Destination $target -Recurse -Force
        Remove-Item -LiteralPath $Snapshot.Backup -Recurse -Force
    }
}

function Get-LogStartupEvents {
    $log = Join-Path (Get-AppDataPath) "logs\furybuilder.log"
    if (-not (Test-Path -LiteralPath $log)) { return @() }
    @(Get-Content -LiteralPath $log -ErrorAction SilentlyContinue | Where-Object { $_ -match '"event":"app\.start"' })
}

function Test-InstalledAppLaunch {
    param([string]$ExePath, [string]$ExpectedVersion)
    if (-not (Test-Path -LiteralPath $ExePath)) {
        throw "Installed executable not found: $ExePath"
    }
    $startedAt = Get-Date
    $before = @(Get-LogStartupEvents).Count
    $process = Start-Process -FilePath $ExePath -PassThru
    $windowSeconds = $null
    $deadline = (Get-Date).AddSeconds(45)
    do {
        Start-Sleep -Milliseconds 500
        $process.Refresh()
        if ($process.HasExited) {
            throw "Installed application exited during startup with code $($process.ExitCode)"
        }
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
            $windowSeconds = [math]::Round(((Get-Date) - $startedAt).TotalSeconds, 2)
            break
        }
    } while ((Get-Date) -lt $deadline)
    if ($null -eq $windowSeconds) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        throw "Installed application did not create a main window within 45 seconds"
    }
    $title = $process.MainWindowTitle
    Start-Sleep -Seconds 2
    $startupEvents = @(Get-LogStartupEvents)
    $newEvents = @($startupEvents | Select-Object -Skip $before)
    $versionMatched = $false
    foreach ($event in $newEvents) {
        if ($event -match ('"appVersion":"' + [regex]::Escape($ExpectedVersion) + '"')) {
            $versionMatched = $true
        }
    }
    if (-not $versionMatched) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        throw "No app.start log record with version $ExpectedVersion after launching the installed application"
    }
    $closedGracefully = $process.CloseMainWindow()
    if (-not $process.WaitForExit(20000)) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $closedGracefully = $false
    }
    if (-not $closedGracefully) {
        throw "Installed application did not close through its main window"
    }
    [pscustomobject]@{
        ok                 = $true
        processId          = $process.Id
        windowTitle        = $title
        windowReadySeconds = $windowSeconds
        startupLogRecords  = $newEvents.Count
        versionMatched     = $versionMatched
        closedGracefully   = $closedGracefully
    }
}

function Invoke-NsisInstall {
    param([string]$InstallerPath)
    Write-Log "Installing NSIS package: $InstallerPath"
    $process = Start-Process -FilePath $InstallerPath -ArgumentList "/S" -PassThru -Wait
    if ($process.ExitCode -ne 0) {
        throw "NSIS installer exited with code $($process.ExitCode)"
    }
    Wait-ForUninstallEntry -TimeoutSeconds 60
}

function Invoke-NsisUninstall {
    $entry = Get-FuryUninstallEntry
    if ($null -eq $entry) { throw "No FuryBuilder installation to uninstall" }
    $installLocation = $entry.InstallLocation
    $installedExe = Get-InstalledExecutable -InstallLocation $installLocation
    $uninstaller = Join-Path $installLocation "uninstall.exe"
    if (-not (Test-Path -LiteralPath $uninstaller)) {
        throw "NSIS uninstaller not found: $uninstaller"
    }
    $entryGone = $false
    $exeGone = $false
    for ($attempt = 1; $attempt -le 2; $attempt++) {
        Write-Log "Uninstalling via $uninstaller /S (attempt $attempt)"
        $process = Start-Process -FilePath $uninstaller -ArgumentList "/S" -PassThru -Wait
        if ($process.ExitCode -ne 0) {
            throw "NSIS uninstaller exited with code $($process.ExitCode)"
        }
        $entryGone = Wait-ForUninstallEntryRemoval -TimeoutSeconds 60
        $exeGone = $true
        if ($null -ne $installedExe) {
            $exeGone = Wait-ForPathRemoval -Path $installedExe -TimeoutSeconds 40
        }
        if ($entryGone -and $exeGone) { break }
        if ($attempt -lt 2) {
            Write-Log "Uninstaller left artifacts behind; retrying once after a short delay"
            Start-Sleep -Seconds 3
        }
    }
    if (-not $entryGone) { throw "Uninstall registry entry remained after uninstall" }
    if (-not $exeGone) { throw "Installed executable remained after uninstall" }
    [pscustomobject]@{
        installLocation      = $installLocation
        executable           = $installedExe
        registryEntryRemoved = $entryGone
        executableRemoved    = $exeGone
    }
}

function Get-FileRecord {
    param([string]$Path, [string]$Format)
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $item = Get-Item -LiteralPath $Path
    [pscustomobject]@{
        format = $Format
        file   = $item.Name
        bytes  = $item.Length
        sha256 = Get-Sha256 -Path $item.FullName
    }
}

$oldPath = Resolve-ExistingPath -Path $OldInstaller -Label "Old NSIS installer"
$newPath = $null
if (-not [string]::IsNullOrWhiteSpace($NewInstaller)) {
    $newPath = Resolve-ExistingPath -Path $NewInstaller -Label "New NSIS installer"
}
$msiPath = $null
if (-not [string]::IsNullOrWhiteSpace($MsiInstaller)) {
    $msiPath = Resolve-ExistingPath -Path $MsiInstaller -Label "MSI package"
}

New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
$oldRecord = Get-FileRecord -Path $oldPath -Format "nsis"
$oldVersion = "unknown"
$oldSetupLeaf = Split-Path -Leaf $oldPath
if ($oldSetupLeaf -match '_(\d+\.\d+\.\d+)_') { $oldVersion = $Matches[1] }
$oldRecord | Add-Member -NotePropertyName version -NotePropertyValue $oldVersion
$script:installers.Add($oldRecord) | Out-Null

if ($null -ne $newPath) {
    $newRecord = Get-FileRecord -Path $newPath -Format "nsis"
    $newVersion = "unknown"
    if ((Split-Path -Leaf $newPath) -match '_(\d+\.\d+\.\d+)_') { $newVersion = $Matches[1] }
    $newRecord | Add-Member -NotePropertyName version -NotePropertyValue $newVersion
    $script:installers.Add($newRecord) | Out-Null
}
if ($null -ne $msiPath) {
    $msiRecord = Get-FileRecord -Path $msiPath -Format "msi"
    $msiVersion = "unknown"
    if ((Split-Path -Leaf $msiPath) -match '_(\d+\.\d+\.\d+)_') { $msiVersion = $Matches[1] }
    $msiRecord | Add-Member -NotePropertyName version -NotePropertyValue $msiVersion
    $script:installers.Add($msiRecord) | Out-Null
}

$machine = [pscustomobject]@{
    computerName   = $env:COMPUTERNAME
    windows        = (Get-CimInstance Win32_OperatingSystem).Caption
    windowsVersion = (Get-CimInstance Win32_OperatingSystem).Version
    webView2       = Get-WebView2Version
    isElevated     = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

$runningApp = Get-Process -Name "furybuilder-desktop" -ErrorAction SilentlyContinue
if ($null -ne $runningApp) {
    throw "FuryBuilder is already running. Close it before running the installer test."
}
$appDataSnapshot = Backup-AppData
$pass = $false
$failure = $null
$exitCode = 1
try {
    Write-Log "Starting NSIS clean install scenario (version $oldVersion)"
    Invoke-NsisInstall -InstallerPath $oldPath
    $entry = Wait-ForUninstallEntry
    if ([string]::IsNullOrWhiteSpace($entry.InstallLocation)) {
        throw "Installer did not record InstallLocation"
    }
    $exe = Get-InstalledExecutable -InstallLocation $entry.InstallLocation
    if ($null -eq $exe) {
        throw "Installer did not produce a known executable in $($entry.InstallLocation)"
    }
    $installedVersion = Get-ExeProductVersion -Path $exe
    if ($installedVersion -ne $oldVersion) {
        throw "Installed executable version $installedVersion does not match bundle version $oldVersion"
    }
    if ($entry.DisplayVersion -ne $oldVersion) {
        throw "Registry DisplayVersion $($entry.DisplayVersion) does not match $oldVersion"
    }
    $script:scenarios.Add([pscustomobject]@{
        name                   = "nsis-clean-install"
        ok                     = $true
        version                = $installedVersion
        installLocation        = $entry.InstallLocation
        registryDisplayVersion = $entry.DisplayVersion
        publisher              = $entry.Publisher
    }) | Out-Null
    Write-Log "Clean install OK: $($entry.InstallLocation) v$installedVersion"

    $launch = Test-InstalledAppLaunch -ExePath $exe -ExpectedVersion $oldVersion
    $script:scenarios.Add([pscustomobject]@{ name = "nsis-installed-app-launch"; ok = $launch.Ok; version = $oldVersion; details = $launch }) | Out-Null
    Write-Log "Installed application launch OK: window in $($launch.WindowReadySeconds) s"

    $markerPath = Join-Path (Get-AppDataPath) "installer-test-marker.txt"
    $markerValue = "furybuilder-installer-test " + [guid]::NewGuid().ToString("N")
    New-Item -ItemType Directory -Path (Get-AppDataPath) -Force | Out-Null
    Set-Content -LiteralPath $markerPath -Value $markerValue -Encoding ASCII
    Write-Log "Wrote upgrade marker: $markerPath"

    if ($null -ne $newPath) {
        $newVersion = "unknown"
        if ((Split-Path -Leaf $newPath) -match '_(\d+\.\d+\.\d+)_') { $newVersion = $Matches[1] }
        Write-Log "Starting NSIS upgrade scenario ($oldVersion -> $newVersion)"
        Invoke-NsisInstall -InstallerPath $newPath
        $entry = Wait-ForUninstallEntry
        $exe = Get-InstalledExecutable -InstallLocation $entry.InstallLocation
        if ($null -eq $exe) {
            throw "Upgrade did not produce a known executable in $($entry.InstallLocation)"
        }
        $upgradedVersion = Get-ExeProductVersion -Path $exe
        if ($upgradedVersion -ne $newVersion) {
            throw "Upgraded executable version $upgradedVersion does not match $newVersion"
        }
        if ($entry.DisplayVersion -ne $newVersion) {
            throw "Registry DisplayVersion $($entry.DisplayVersion) does not match $newVersion"
        }
        if (-not (Test-Path -LiteralPath $markerPath)) {
            throw "Upgrade removed the application data marker"
        }
        $markerAfter = (Get-Content -LiteralPath $markerPath -Raw).Trim()
        if ($markerAfter -ne $markerValue) {
            throw "Upgrade changed the application data marker"
        }
        $launch = Test-InstalledAppLaunch -ExePath $exe -ExpectedVersion $newVersion
        $script:scenarios.Add([pscustomobject]@{
            name                   = "nsis-upgrade"
            ok                     = $true
            fromVersion            = $oldVersion
            toVersion              = $newVersion
            installLocation        = $entry.InstallLocation
            registryDisplayVersion = $entry.DisplayVersion
            appDataMarkerPreserved = $true
            launch                 = $launch
        }) | Out-Null
        Write-Log "Upgrade OK: v$upgradedVersion, marker preserved, app launched"
    }
    else {
        $script:scenarios.Add([pscustomobject]@{ name = "nsis-upgrade"; ok = $false; skipped = $true; reason = "No -NewInstaller supplied" }) | Out-Null
        $script:limitations.Add("NSIS upgrade was not exercised because no second installer version was supplied.")
        Write-Log "Upgrade scenario skipped"
    }

    Write-Log "Starting NSIS uninstall scenario"
    $uninstall = Invoke-NsisUninstall
    $dataSurvived = Test-Path -LiteralPath $markerPath
    $script:scenarios.Add([pscustomobject]@{
        name                 = "nsis-uninstall"
        ok                   = $true
        registryEntryRemoved = $uninstall.RegistryEntryRemoved
        executableRemoved    = $uninstall.ExecutableRemoved
        installLocation      = $uninstall.InstallLocation
        appDataPreserved     = $dataSurvived
    }) | Out-Null
    Write-Log "Uninstall OK: executable and registry entry removed, app data present=$dataSurvived"

    if ($null -ne $msiPath) {
        Write-Log "Starting MSI administrative extraction scenario"
        $extractDir = Join-Path $env:TEMP ("furybuilder-msi-admin-" + [guid]::NewGuid().ToString("N"))
        New-Item -ItemType Directory -Path $extractDir -Force | Out-Null
        $msiArgs = "/a `"$msiPath`" /qn TARGETDIR=`"$extractDir`""
        $msiProcess = Start-Process -FilePath "msiexec.exe" -ArgumentList $msiArgs -PassThru -Wait
        if ($msiProcess.ExitCode -ne 0) {
            throw "msiexec /a exited with code $($msiProcess.ExitCode)"
        }
        $extractedExe = Get-ChildItem -LiteralPath $extractDir -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -in @("FuryBuilder.exe", "furybuilder-desktop.exe") } |
            Select-Object -First 1
        if ($null -eq $extractedExe) {
            throw "MSI administrative extraction did not contain a known FuryBuilder executable"
        }
        $msiVersion = Get-ExeProductVersion -Path $extractedExe.FullName
        $script:scenarios.Add([pscustomobject]@{
            name                      = "msi-package-extraction"
            ok                        = $true
            mode                      = "administrative extraction (msiexec /a); per-machine install requires elevation"
            extractedVersion          = $msiVersion
            extractedExecutableSha256 = Get-Sha256 -Path $extractedExe.FullName
        }) | Out-Null
        $script:limitations.Add("The MSI was validated by administrative extraction only. A silent per-machine install needs an elevated session, which this non-elevated harness does not request.")
        Remove-Item -LiteralPath $extractDir -Recurse -Force -ErrorAction SilentlyContinue
        Write-Log "MSI extraction OK: v$msiVersion"
    }

    $pass = $true
    $exitCode = 0
}
catch {
    $failure = $_
    Write-Log ("FAILED: " + $_.Exception.Message)
    try {
        $entry = Get-FuryUninstallEntry
        if ($null -ne $entry) {
            Write-Log "Cleaning up the failed installation"
            Invoke-NsisUninstall | Out-Null
        }
    }
    catch {
        Write-Log ("Cleanup after failure also failed: " + $_.Exception.Message)
    }
}
finally {
    Restore-AppData -Snapshot $appDataSnapshot
}

Write-Log "Writing evidence"
$evidence = [ordered]@{}
$evidence["schemaVersion"] = 1
$evidence["milestone"] = "M11"
$evidence["task"] = "windows-installer"
$evidence["recordedAt"] = (Get-Date).ToString("o")
$evidence["machine"] = $machine
$evidence["installers"] = $script:installers.ToArray()
$evidence["scenarios"] = $script:scenarios.ToArray()
$evidence["limitations"] = $script:limitations.ToArray()
$evidence["appDataBackedUpAndRestored"] = $true
$evidence["pass"] = $pass
if ($null -ne $failure) {
    $evidence["failure"] = $failure.Exception.Message
}
else {
    $evidence["failure"] = $null
}

$evidencePath = Join-Path $OutDir "installer-evidence.json"
$logPath = Join-Path $OutDir "installer-test.log"
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText([System.IO.Path]::GetFullPath($evidencePath), ($evidence | ConvertTo-Json -Depth 12), $utf8NoBom)
[System.IO.File]::WriteAllText([System.IO.Path]::GetFullPath($logPath), (($script:lines -join [Environment]::NewLine) + [Environment]::NewLine), $utf8NoBom)
Write-Host "Evidence: $evidencePath"
Write-Host "Transcript: $logPath"
exit $exitCode
