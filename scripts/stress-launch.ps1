<#
.SYNOPSIS
  Opens and closes Osprey many times on Windows and reports what looks off:
  slow or failed starts, hangs on close, crashes, processes left behind,
  idle CPU, memory and handle counts.

.DESCRIPTION
  Every run starts the exe with OSPREY_APP_DIR pointing at a throwaway
  folder, so the settings of the copy you use every day are never touched. The tools
  (yt-dlp, ffmpeg, deno) are reused through a junction, so nothing of them is
  copied or downloaded per run.

  The app is closed the way a user does, by asking its window to close. A
  run fails when the window never shows, the app doesn't exit, it exits with
  an error, Windows logs a crash for it, or a child process outlives it.

  Windows open and take the focus while this runs; leave the machine alone.

.EXAMPLE
  # The release build of this checkout, with the tools of an unpacked release
  npx tauri build --no-bundle
  .\scripts\stress-launch.ps1 -Tools C:\Users\me\Downloads\osprey-0.9.0-windows-x86_64

.EXAMPLE
  # A downloaded release, every run from an empty settings folder
  .\scripts\stress-launch.ps1 -Exe C:\Users\PC\Downloads\osprey-0.9.0-windows-x86_64\osprey.exe -State cold -Runs 50

.EXAMPLE
  # One long session, memory and handles every 5 seconds, to look for leaks
  .\scripts\stress-launch.ps1 -SoakMinutes 30
#>
[CmdletBinding()]
param(
    # Default: the release build of this checkout.
    [string]$Exe,
    # A folder holding bin\ with the tools. Default: the one next to the exe.
    [string]$Tools,
    [int]$Runs = 30,
    # How long each run stays open before it is closed.
    [int]$SettleSeconds = 4,
    # warm: one settings folder for every run. cold: a new one each run.
    [ValidateSet('warm', 'cold')][string]$State = 'warm',
    [int]$StartTimeoutSeconds = 20,
    [int]$CloseTimeoutSeconds = 10,
    # Above 0: a single long run that samples memory instead of the loop.
    [int]$SoakMinutes = 0,
    [string]$OutDir = (Join-Path $env:TEMP 'osprey-stress')
)

$ErrorActionPreference = 'Stop'
if (-not $Exe) { $Exe = Join-Path (Split-Path -Parent $PSCommandPath) '..\target\release\osprey.exe' }
$Exe = (Resolve-Path $Exe).Path
if (-not $Tools) { $Tools = Split-Path $Exe }
$toolsBin = Join-Path $Tools 'bin'
if (-not (Test-Path (Join-Path $toolsBin 'yt-dlp.exe'))) {
    Write-Warning "No tools in ${toolsBin}: the app will install them, and every cold run again. Pass -Tools <a folder with bin\>."
}
$name = [IO.Path]::GetFileNameWithoutExtension($Exe)
$cores = [Environment]::ProcessorCount

if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force }
New-Item -ItemType Directory -Path $OutDir | Out-Null
$stamp = Get-Date

function New-RunDir([string]$label) {
    $dir = Join-Path $OutDir $label
    New-Item -ItemType Directory -Path $dir | Out-Null
    if (Test-Path $toolsBin) {
        New-Item -ItemType Junction -Path (Join-Path $dir 'bin') -Target $toolsBin | Out-Null
    }
    $dir
}

# Everything that descends from $rootId, with its name.
function Get-Descendants([int]$rootId) {
    $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name)
    $found = @()
    $queue = [Collections.Generic.Queue[int]]::new()
    $queue.Enqueue($rootId)
    while ($queue.Count) {
        $parent = $queue.Dequeue()
        foreach ($child in $all | Where-Object { $_.ParentProcessId -eq $parent -and $_.ProcessId -ne $parent }) {
            $found += $child
            $queue.Enqueue([int]$child.ProcessId)
        }
    }
    $found
}

function Start-App([string]$dir) {
    $env:OSPREY_APP_DIR = $dir
    try { Start-Process -FilePath $Exe -PassThru }
    finally { Remove-Item Env:\OSPREY_APP_DIR -ErrorAction SilentlyContinue }
}

# Waits for the window to be on screen; returns milliseconds, or $null.
function Wait-Window($proc, [int]$timeoutSeconds) {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    while ($watch.Elapsed.TotalSeconds -lt $timeoutSeconds) {
        if ($proc.HasExited) { return $null }
        $proc.Refresh()
        # On Windows the app starts hidden, and a hidden helper window comes
        # first, with no title. The real one has it.
        if ($proc.MainWindowHandle -ne [IntPtr]::Zero -and $proc.MainWindowTitle) {
            return [int]$watch.ElapsedMilliseconds
        }
        Start-Sleep -Milliseconds 20
    }
    $null
}

function Get-Percentile([double[]]$values, [double]$p) {
    if (-not $values.Count) { return $null }
    $sorted = $values | Sort-Object
    $sorted[[math]::Min($sorted.Count - 1, [math]::Floor($p * $sorted.Count))]
}

function Format-Stats([string]$label, [double[]]$values, [string]$unit) {
    if (-not $values.Count) { return "{0,-26} (no data)" -f $label }
    "{0,-26} min {1,7:N0}   median {2,7:N0}   p95 {3,7:N0}   max {4,7:N0} {5}" -f $label,
        ($values | Measure-Object -Minimum).Minimum, (Get-Percentile $values 0.5),
        (Get-Percentile $values 0.95), ($values | Measure-Object -Maximum).Maximum, $unit
}

# --------------------------------------------------------------- soak mode
if ($SoakMinutes -gt 0) {
    $dir = New-RunDir 'soak'
    $app = Start-App $dir
    if ($null -eq (Wait-Window $app $StartTimeoutSeconds)) { throw "The window never showed." }
    $csv = Join-Path $OutDir 'soak.csv'
    $rows = @()
    $end = (Get-Date).AddMinutes($SoakMinutes)
    Write-Host "Soak: $SoakMinutes min, a sample every 5 s -> $csv"
    while ((Get-Date) -lt $end -and -not $app.HasExited) {
        $app.Refresh()
        $rows += [pscustomobject]@{
            Seconds   = [int]((Get-Date) - $stamp).TotalSeconds
            WorkingMB = [math]::Round($app.WorkingSet64 / 1MB, 1)
            PrivateMB = [math]::Round($app.PrivateMemorySize64 / 1MB, 1)
            Handles   = $app.HandleCount
            Threads   = $app.Threads.Count
            CpuSec    = [math]::Round($app.TotalProcessorTime.TotalSeconds, 2)
            Children  = @(Get-Descendants $app.Id).Count
        }
        Start-Sleep -Seconds 5
    }
    $rows | Export-Csv $csv -NoTypeInformation
    if ($app.HasExited) { Write-Host "FAIL: the app exited by itself with code $($app.ExitCode)."; exit 1 }
    [void]$app.CloseMainWindow()
    if (-not $app.WaitForExit($CloseTimeoutSeconds * 1000)) { $app.Kill(); Write-Host "FAIL: it did not close." }
    $first = $rows[[math]::Min(2, $rows.Count - 1)]
    $last = $rows[-1]
    "Private memory {0} MB -> {1} MB, handles {2} -> {3}, threads {4} -> {5}" -f
        $first.PrivateMB, $last.PrivateMB, $first.Handles, $last.Handles, $first.Threads, $last.Threads
    "A line that keeps climbing in $csv is a leak; a plateau is not."
    exit 0
}

# ------------------------------------------------------------ the main loop
$results = @()
$problems = @()
$sharedDir = if ($State -eq 'warm') { New-RunDir 'state' }

Write-Host "$Runs runs of $Exe ($State state, $SettleSeconds s open each)"
for ($i = 1; $i -le $Runs; $i++) {
    $dir = if ($State -eq 'warm') { $sharedDir } else { New-RunDir ('run{0:000}' -f $i) }
    $began = Get-Date
    $row = [ordered]@{ Run = $i; StartMs = $null; IdleCpuPct = $null; WorkingMB = $null; PrivateMB = $null
        PeakMB = $null; Handles = $null; Threads = $null; Children = 0; CloseMs = $null; ExitCode = $null; Notes = '' }
    $notes = @()

    $app = Start-App $dir
    $row.StartMs = Wait-Window $app $StartTimeoutSeconds
    if ($null -eq $row.StartMs) {
        $notes += if ($app.HasExited) { "exited at start (code $($app.ExitCode))" } else { 'no window' }
    }

    if (-not $app.HasExited) {
        Start-Sleep -Seconds ([math]::Max(1, $SettleSeconds - 2))
        $app.Refresh(); $cpu0 = $app.TotalProcessorTime.TotalSeconds; $t0 = Get-Date
        Start-Sleep -Seconds 2
        $app.Refresh(); $cpu1 = $app.TotalProcessorTime.TotalSeconds
        $row.IdleCpuPct = [math]::Round(($cpu1 - $cpu0) / ((Get-Date) - $t0).TotalSeconds / $cores * 100, 2)
        $row.WorkingMB = [math]::Round($app.WorkingSet64 / 1MB, 1)
        $row.PrivateMB = [math]::Round($app.PrivateMemorySize64 / 1MB, 1)
        $row.Handles = $app.HandleCount
        $row.Threads = $app.Threads.Count
        $children = @(Get-Descendants $app.Id)
        $row.Children = $children.Count

        $closing = [Diagnostics.Stopwatch]::StartNew()
        [void]$app.CloseMainWindow()
        if ($app.WaitForExit($CloseTimeoutSeconds * 1000)) {
            $row.CloseMs = [int]$closing.ElapsedMilliseconds
        } else {
            $notes += "HANG: still open $CloseTimeoutSeconds s after closing"
            $app.Kill(); $app.WaitForExit()
        }
    }
    $row.ExitCode = $app.ExitCode

    # What it left behind.
    if ($row.ExitCode -ne 0 -and $notes -notmatch 'HANG') { $notes += "exit code $($row.ExitCode)" }
    Start-Sleep -Milliseconds 300
    $alive = @($children | Where-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue })
    if ($alive.Count) { $notes += 'ORPHANS: ' + (($alive | ForEach-Object { "$($_.Name)#$($_.ProcessId)" }) -join ', ') }
    $leftovers = @(Get-ChildItem $dir -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '\.(tmp|part|new|lock|bak)$' })
    if ($leftovers.Count) { $notes += 'leftover files: ' + (($leftovers.Name) -join ', ') }
    try {
        $crashes = @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; StartTime = $began
                ProviderName = 'Application Error', 'Windows Error Reporting' } -ErrorAction Stop |
            Where-Object { $_.Message -match [regex]::Escape($name) })
        if ($crashes.Count) { $notes += 'CRASH logged by Windows' }
    } catch [System.Exception] {
        if ($_.FullyQualifiedErrorId -notmatch 'NoMatchingEventsFound') { $notes += "event log: $($_.Exception.Message)" }
    }

    $row.Notes = $notes -join '; '
    $results += [pscustomobject]$row
    if ($notes.Count) { $problems += "run ${i}: $($row.Notes)" }
    Write-Host ("{0,3}/{1}  start {2,5} ms  idle cpu {3,5}%  mem {4,6} MB  close {5,5} ms  {6}" -f
        $i, $Runs, $row.StartMs, $row.IdleCpuPct, $row.PrivateMB, $row.CloseMs, $(if ($notes.Count) { '<-- ' + $row.Notes }))
}

$csv = Join-Path $OutDir 'runs.csv'
$results | Export-Csv $csv -NoTypeInformation

function Column($prop) { [double[]]@($results | Where-Object { $null -ne $_.$prop } | ForEach-Object { $_.$prop }) }
Write-Host ''
Write-Host (Format-Stats 'Window shown after' (Column 'StartMs') 'ms')
Write-Host (Format-Stats 'Closed after' (Column 'CloseMs') 'ms')
Write-Host (Format-Stats 'Idle CPU (x100 %)' ([double[]]@(Column 'IdleCpuPct' | ForEach-Object { $_ * 100 })) '')
Write-Host (Format-Stats 'Private memory' (Column 'PrivateMB') 'MB')
Write-Host (Format-Stats 'Handles' (Column 'Handles') '')
Write-Host (Format-Stats 'Threads' (Column 'Threads') '')

# Odd ones out, without a fixed threshold: far from the median.
$starts = Column 'StartMs'
if ($starts.Count -ge 5) {
    $median = Get-Percentile $starts 0.5
    $slow = @($results | Where-Object { $_.StartMs -gt [math]::Max(3 * $median, $median + 1500) })
    foreach ($r in $slow) { $problems += "run $($r.Run): slow start, $($r.StartMs) ms (median $median ms)" }
}
$busy = @($results | Where-Object { $_.IdleCpuPct -gt 5 })
foreach ($r in $busy) { $problems += "run $($r.Run): $($r.IdleCpuPct)% CPU while idle (a redraw loop?)" }

Write-Host ''
Write-Host "Detail: $csv"
if ($problems.Count) {
    Write-Host "`n$($problems.Count) thing(s) to look at:" -ForegroundColor Yellow
    $problems | ForEach-Object { Write-Host "  - $_" -ForegroundColor Yellow }
    exit 1
}
Write-Host 'No problems found.' -ForegroundColor Green
