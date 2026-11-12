# Before/after benchmark of the egui and Win32 Windows apps on one machine.
# Each run starts one app with --demo and measures:
#   - time from process start to its first visible window;
#   - working set and private bytes after opening the demo `users` table;
#   - CPU (% of one core) while idle with that table open;
#   - CPU while scrolling the grid with synthesized wheel input, plus the
#     Win32 app's presented frame rate (it writes a frame count to
#     DBM_FRAME_LOG; egui has no equivalent).
# Runs alternate between the apps; the summary reports medians.
param(
    [Parameter(Mandatory = $true)] [string]$Egui,
    [Parameter(Mandatory = $true)] [string]$Win32,
    [int]$Runs = 5,
    [int]$IdleSeconds = 60,
    [int]$ScrollSeconds = 15,
    [string]$Out = "bench-results"
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Native {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, int data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool repaint);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
    // The process's visible top-level window with this title (winit also
    // creates other windows, so MainWindowHandle can be the wrong one).
    public static IntPtr FindAppWindow(int pid, string title) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p != pid || !IsWindowVisible(h)) return true;
            var sb = new System.Text.StringBuilder(256);
            GetWindowText(h, sb, 256);
            if (sb.ToString() == title) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path

function Save-Screen([IntPtr]$hwnd, [string]$path) {
    $r = New-Object Native+RECT
    [Native]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    if ($r.Right -le $r.Left) { return }
    $bmp = New-Object Drawing.Bitmap ($r.Right - $r.Left), ($r.Bottom - $r.Top)
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
    $bmp.Save($path, [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

function Dump-Tree([IntPtr]$hwnd, [string]$path) {
    try {
        $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
        $all = $root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
        $all | Select-Object -First 300 | ForEach-Object {
            "$($_.Current.ControlType.ProgrammaticName)`t$($_.Current.Name)`t$($_.Current.BoundingRectangle)"
        } | Set-Content $path
    } catch { "UIA failed: $_" | Set-Content $path }
}

function Click-At([int]$x, [int]$y) {
    [Native]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 60
    [Native]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [Native]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
}

function Find-Element([IntPtr]$hwnd, [string]$name, [int]$timeoutSeconds = 30) {
    $cond = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $name)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt $timeoutSeconds) {
        try {
            $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
            $e = $root.FindFirst([Windows.Automation.TreeScope]::Descendants, $cond)
            if ($e -and -not $e.Current.BoundingRectangle.IsEmpty) { return $e }
        } catch { }
        Start-Sleep -Milliseconds 250
    }
    $tag = Get-Random
    Save-Screen $hwnd (Join-Path $Out "missing-$tag.png")
    Dump-Tree $hwnd (Join-Path $Out "missing-$tag-uia.txt")
    throw "No UI Automation element named '$name' after $timeoutSeconds s (see missing-$tag.*)"
}

function Click-Element([IntPtr]$hwnd, [string]$name) {
    $r = (Find-Element $hwnd $name).Current.BoundingRectangle
    Click-At ([int]($r.X + $r.Width / 2)) ([int]($r.Y + $r.Height / 2))
}

function Cpu-Time($p) { $p.Refresh(); $p.TotalProcessorTime.TotalMilliseconds }

function Measure-App([string]$label, [string]$exe, [int]$run) {
    Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($exe)) -ErrorAction SilentlyContinue | Stop-Process -Force
    # The egui demo keeps its layout in a temp file; start every run fresh.
    Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $env:TEMP 'dbm-demo-layout.ron')
    $frameLog = Join-Path $Out "frames-$label-$run.txt"
    Remove-Item -Force -ErrorAction SilentlyContinue $frameLog
    $env:DBM_FRAME_LOG = $frameLog
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath $exe -ArgumentList '--demo' -PassThru
    $hwnd = [IntPtr]::Zero
    while ($sw.Elapsed.TotalSeconds -lt 60) {
        $hwnd = [Native]::FindAppWindow($p.Id, "Anybase $([char]0x2014) DEMO")
        if ($hwnd -ne [IntPtr]::Zero) { break }
        $p.Refresh()
        if ($p.HasExited) { break }
        Start-Sleep -Milliseconds 5
    }
    $firstWindowMs = $sw.Elapsed.TotalMilliseconds
    if ($hwnd -eq [IntPtr]::Zero -or -not [Native]::IsWindowVisible($hwnd)) {
        $p.Refresh()
        $why = if ($p.HasExited) { "exited with $($p.ExitCode)" } else { 'still running' }
        $log = Join-Path $env:TEMP 'anybase-win32-error.txt'
        if (Test-Path $log) { $why += ": " + (Get-Content $log -Raw) }
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        throw "$label showed no window ($why)"
    }
    try {
        # Same place and size for both apps.
        [Native]::MoveWindow($hwnd, 0, 0, 1280, 800, $true) | Out-Null
        [Native]::SetForegroundWindow($hwnd) | Out-Null
        Start-Sleep -Seconds 2
        Click-Element $hwnd 'Production'
        Click-Element $hwnd 'users'
        Start-Sleep -Seconds 3
        if ($run -eq 1) { Save-Screen $hwnd (Join-Path $Out "$label-table.png") }
        $p.Refresh()
        $workingSet = $p.WorkingSet64
        $private = $p.PrivateMemorySize64
        $wr = New-Object Native+RECT
        [Native]::GetWindowRect($hwnd, [ref]$wr) | Out-Null
        # Park the pointer on the title bar so hover doesn't count as work.
        [Native]::SetCursorPos($wr.Left + 640, $wr.Top + 12) | Out-Null
        Start-Sleep -Seconds 1
        $c0 = Cpu-Time $p
        $t0 = [Diagnostics.Stopwatch]::StartNew()
        Start-Sleep -Seconds $IdleSeconds
        $idleCpu = ((Cpu-Time $p) - $c0) / $t0.Elapsed.TotalMilliseconds * 100
        # Scroll over the grid: one wheel notch every ~16 ms, reversing every 15.
        [Native]::SetCursorPos($wr.Left + 760, $wr.Top + 520) | Out-Null
        Start-Sleep -Milliseconds 300
        $f0 = if (Test-Path $frameLog) { [int](Get-Content $frameLog) } else { 0 }
        $c0 = Cpu-Time $p
        $t0 = [Diagnostics.Stopwatch]::StartNew()
        $i = 0
        while ($t0.Elapsed.TotalSeconds -lt $ScrollSeconds) {
            $delta = if ([math]::Floor($i / 15) % 2 -eq 0) { -120 } else { 120 }
            [Native]::mouse_event(0x0800, 0, 0, $delta, [UIntPtr]::Zero)
            $i++
            Start-Sleep -Milliseconds 16
        }
        $elapsed = $t0.Elapsed.TotalMilliseconds
        $scrollCpu = ((Cpu-Time $p) - $c0) / $elapsed * 100
        Start-Sleep -Milliseconds 300
        $fps = $null
        if (Test-Path $frameLog) { $fps = ([int](Get-Content $frameLog) - $f0) / (($elapsed + 300) / 1000) }
        [pscustomobject]@{
            app = $label; run = $run
            firstWindowMs = [math]::Round($firstWindowMs, 1)
            workingSetMB = [math]::Round($workingSet / 1MB, 1)
            privateMB = [math]::Round($private / 1MB, 1)
            idleCpuPct = [math]::Round($idleCpu, 2)
            scrollCpuPct = [math]::Round($scrollCpu, 1)
            scrollFps = if ($null -ne $fps) { [math]::Round($fps, 1) } else { $null }
            wheelNotches = $i
        }
    } finally {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
    }
}

Add-Type -AssemblyName System.Windows.Forms
Write-Host "Screen: $([Windows.Forms.Screen]::PrimaryScreen.Bounds)"
$results = @()
$errors = @()
for ($run = 1; $run -le $Runs; $run++) {
    foreach ($app in @(@{ label = 'egui'; exe = $Egui }, @{ label = 'win32'; exe = $Win32 })) {
        try {
            $r = Measure-App $app.label $app.exe $run
            Write-Host ($r | ConvertTo-Json -Compress)
            $results += $r
        } catch {
            $msg = "$($app.label) run ${run}: $_"
            Write-Warning $msg
            $errors += $msg
        }
    }
}

function Median($values) {
    $v = @($values | Where-Object { $null -ne $_ } | Sort-Object)
    if ($v.Count -eq 0) { return $null }
    if ($v.Count % 2) { return $v[[int][math]::Floor($v.Count / 2)] }
    return ($v[$v.Count / 2 - 1] + $v[$v.Count / 2]) / 2
}

$metrics = [ordered]@{
    firstWindowMs = 'Time to first window (ms)'
    workingSetMB  = 'Working set, table open (MB)'
    privateMB     = 'Private bytes, table open (MB)'
    idleCpuPct    = "Idle CPU over ${IdleSeconds} s (% of one core)"
    scrollCpuPct  = 'CPU while wheel-scrolling the grid (% of one core)'
    scrollFps     = 'Frames presented while scrolling (fps)'
}
$medians = [ordered]@{}
$lines = @(
    "| Metric (median of $Runs runs) | egui (wgpu) | Win32 (Direct2D) |",
    '| --- | ---: | ---: |'
)
foreach ($key in $metrics.Keys) {
    $e = Median ($results | Where-Object app -eq 'egui' | ForEach-Object { $_.$key })
    $w = Median ($results | Where-Object app -eq 'win32' | ForEach-Object { $_.$key })
    $medians[$key] = [ordered]@{ egui = $e; win32 = $w }
    $fmt = { param($x) if ($null -eq $x) { 'n/a' } else { [string][math]::Round($x, 2) } }
    $lines += "| $($metrics[$key]) | $(& $fmt $e) | $(& $fmt $w) |"
}
$lines += ''
$lines += "Runner: $((Get-CimInstance Win32_OperatingSystem).Caption), $([Environment]::ProcessorCount) logical CPUs, no GPU (both apps fall back to software rendering: WARP)."
if ($errors.Count) { $lines += ''; $lines += 'Errors:'; $errors | ForEach-Object { $lines += "- $_" } }

[ordered]@{ runs = $results; medians = $medians; errors = $errors } | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Out 'results.json')
$lines -join "`n" | Set-Content (Join-Path $Out 'summary.md')
$lines -join "`n" | Write-Host
if ($env:GITHUB_STEP_SUMMARY) { $lines -join "`n" | Add-Content $env:GITHUB_STEP_SUMMARY }
if ($results.Count -eq 0) { exit 1 }
