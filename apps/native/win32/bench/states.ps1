# Resource use of the Win32 app, state by state, against a live PostgreSQL.
#
# Drives the app only through UI Automation (Invoke and Value patterns), plus
# wheel messages posted to its own window, so it never moves the cursor or
# takes the foreground and can run on a machine someone else is using. Each
# run starts with a fresh profile store: USERPROFILE, APPDATA, LOCALAPPDATA
# and TEMP point into -WorkDir, so real profiles, credentials and settings are
# never read or written (the connection has no password, so nothing reaches
# Credential Manager either).
#
# The database needs no password and holds these tables:
#   events (id bigserial primary key, created_at timestamptz, user_id int,
#           kind text, title text, body text, payload jsonb, score numeric)
#           with a few hundred thousand rows of wide text (see docs/native.md);
#   users and products, any small tables.
#
# Records working set, private bytes, GPU dedicated/shared memory, handles
# and threads per state into OUT\states.csv, and launch, idle and scrolling
# timings into OUT\timing.json. CPU timings are skipped while any cargo,
# rustc, link, cl or msbuild process runs, or with -WaitForQuiet wait for
# them to finish.
#
# UI Automation's Invoke activates the window it targets, so on a machine
# others are using, run this on a separate desktop (CreateDesktop, then start
# PowerShell there with STARTUPINFO.lpDesktop); the app inherits it.
param(
    [Parameter(Mandatory = $true)] [string]$Exe,
    [string]$Out = 'bench-states',
    [string]$WorkDir = (Join-Path $env:TEMP 'anybase-bench-profile'),
    [string]$PgHost = '127.0.0.1',
    [int]$PgPort = 5432,
    [string]$PgUser = 'postgres',
    [string]$PgDatabase = 'bench',
    [int]$IdleSeconds = 60,
    [int]$CpuSeconds = 20,
    [int]$ScrollSeconds = 15,
    # Wait for compilers to finish before each timed step instead of skipping it.
    [switch]$WaitForQuiet
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class Bench {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct STARTUPINFO { public int cb; public string r, d, t; public int x, y, xs, ys, xc, yc, fill, flags; public short show, r2; public IntPtr r3, i, o, e; }
    [StructLayout(LayoutKind.Sequential)] struct PI { public IntPtr hp, ht; public int pid, tid; }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CreateProcessW(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string cwd, ref STARTUPINFO si, out PI pi);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    // Starts the app shown without activation (SW_SHOWNOACTIVATE).
    public static int Launch(string exe, string cwd) {
        var si = new STARTUPINFO(); si.cb = Marshal.SizeOf(si); si.flags = 1; si.show = 4;
        PI pi;
        if (!CreateProcessW(exe, new StringBuilder("\"" + exe + "\""), IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, cwd, ref si, out pi))
            throw new Exception("CreateProcess failed " + Marshal.GetLastWin32Error());
        CloseHandle(pi.hp); CloseHandle(pi.ht); return pi.pid;
    }
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint flags);
    [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    public static IntPtr FindAppWindow(int pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p != pid || !IsWindowVisible(h)) return true;
            var sb = new StringBuilder(64); GetClassName(h, sb, 64);
            if (sb.ToString() == "AnybaseWindow") { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
    // A wheel notch posted to the window itself: no cursor move, no input queue.
    public static void Wheel(IntPtr h, int sx, int sy, int delta) {
        long l = (long)(((uint)(sy & 0xffff) << 16) | (uint)(sx & 0xffff));
        long w = (long)((uint)(delta & 0xffff) << 16);
        PostMessageW(h, 0x020A, new IntPtr(w), new IntPtr(l));
    }
    [StructLayout(LayoutKind.Sequential)] struct PMC { public uint cb, PageFaultCount; public UIntPtr Peak, WS, a, b, c, d, Pagefile, PeakPagefile, Private; }
    [DllImport("psapi.dll")] static extern bool GetProcessMemoryInfo(IntPtr p, out PMC c, uint cb);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint a, bool i, int pid);
    // Working set and private bytes (commit).
    public static ulong[] Mem(int pid) {
        IntPtr p = OpenProcess(0x1000 | 0x0010, false, pid); PMC c;
        GetProcessMemoryInfo(p, out c, (uint)Marshal.SizeOf(typeof(PMC))); CloseHandle(p);
        return new ulong[] { (ulong)c.WS, (ulong)c.Private };
    }
}
"@
# Physical pixels for UI Automation rectangles and monitor bounds.
[void][Bench]::SetProcessDpiAwarenessContext([IntPtr](-4))

New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$Exe = (Resolve-Path $Exe).Path
if (Test-Path $WorkDir) { Remove-Item -Recurse -Force $WorkDir }
foreach ($d in 'AppData\Roaming', 'AppData\Local\Temp') { New-Item -ItemType Directory -Force (Join-Path $WorkDir $d) | Out-Null }
# dbm-core's store is the LocalAppData known folder, which follows USERPROFILE.
$env:USERPROFILE = $WorkDir
$env:APPDATA = Join-Path $WorkDir 'AppData\Roaming'
$env:LOCALAPPDATA = Join-Path $WorkDir 'AppData\Local'
$env:TEMP = Join-Path $WorkDir 'AppData\Local\Temp'; $env:TMP = $env:TEMP
$frameLog = Join-Path $Out 'frames.txt'
Remove-Item $frameLog -ErrorAction SilentlyContinue
$env:DBM_FRAME_LOG = $frameLog

function Frames { if (Test-Path $frameLog) { [int](Get-Content $frameLog) } else { 0 } }
function Compiling { [bool](Get-Process -Name cargo, rustc, link, cl, msbuild -ErrorAction SilentlyContinue) }
# Whether a timed step may run now.
function Quiet {
    if ($WaitForQuiet) { while (Compiling) { Start-Sleep 5 } }
    -not (Compiling)
}
function Cpu-Ms { (Get-Process -Id $script:procId).TotalProcessorTime.TotalMilliseconds }
function Ctx-Switches {
    (Get-CimInstance Win32_PerfRawData_PerfProc_Thread -Filter "IDProcess = $script:procId" | Measure-Object ContextSwitchesPersec -Sum).Sum
}
function Root { [Windows.Automation.AutomationElement]::FromHandle($script:hwnd) }
function Find-El([string]$name, [string]$type, [int]$timeout = 20) {
    $cond = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $name)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt $timeout) {
        try {
            foreach ($e in (Root).FindAll([Windows.Automation.TreeScope]::Descendants, $cond)) {
                if (-not $type -or $e.Current.ControlType.ProgrammaticName -eq "ControlType.$type") { return $e }
            }
        } catch { }
        Start-Sleep -Milliseconds 150
    }
    throw "no element '$name'"
}
function Find-Match([string]$regex, [int]$timeout = 20) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt $timeout) {
        try {
            foreach ($e in (Root).FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)) {
                if ($e.Current.Name -match $regex) { return $e }
            }
        } catch { }
        Start-Sleep -Milliseconds 150
    }
    throw "no element matching '$regex'"
}
function Invoke-El([string]$name, [string]$type) {
    (Find-El $name $type).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    Start-Sleep -Milliseconds 120
}
function Set-El([string]$name, [string]$value) {
    (Find-El $name 'Edit').GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($value)
    Start-Sleep -Milliseconds 120
}
function Gpu-Mem {
    try {
        $s = (Get-Counter "\GPU Process Memory(pid_$($script:procId)_*)\Dedicated Usage", "\GPU Process Memory(pid_$($script:procId)_*)\Shared Usage" -ErrorAction Stop).CounterSamples
        @(($s | Where-Object Path -like '*dedicated usage' | Measure-Object CookedValue -Sum).Sum,
          ($s | Where-Object Path -like '*shared usage' | Measure-Object CookedValue -Sum).Sum)
    } catch { @($null, $null) }
}
$rows = @()
function Sample([string]$state) {
    Start-Sleep -Milliseconds 300
    $m = [Bench]::Mem($script:procId); $g = Gpu-Mem; $p = Get-Process -Id $script:procId
    $r = [pscustomobject]@{
        state = $state
        wsMB = [math]::Round($m[0] / 1MB, 1); privateMB = [math]::Round($m[1] / 1MB, 1)
        gpuDedMB = if ($null -ne $g[0]) { [math]::Round($g[0] / 1MB, 1) } else { $null }
        gpuSharedMB = if ($null -ne $g[1]) { [math]::Round($g[1] / 1MB, 1) } else { $null }
        handles = $p.HandleCount; threads = $p.Threads.Count
    }
    $script:rows += $r
    Write-Host ($r | ConvertTo-Json -Compress)
}
function Run-Query([string]$sql) {
    Set-El 'SQL editor' $sql; Invoke-El 'Run statement'
    [void](Find-Match 'Query completed.*10,000 rows' 30)
    Start-Sleep 1
}

$timing = [ordered]@{}
$sw = [Diagnostics.Stopwatch]::StartNew()
$script:procId = [Bench]::Launch($Exe, $WorkDir)
$script:hwnd = [IntPtr]::Zero
while ($sw.Elapsed.TotalSeconds -lt 30 -and $script:hwnd -eq [IntPtr]::Zero) {
    $script:hwnd = [Bench]::FindAppWindow($script:procId); Start-Sleep -Milliseconds 2
}
$timing.firstWindowMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 1)
if ($script:hwnd -eq [IntPtr]::Zero) { throw 'no window' }
try {
    # Bottom of the z-order, without activating.
    [void][Bench]::SetWindowPos($script:hwnd, [IntPtr]1, 0, 0, 0, 0, 0x13)
    Start-Sleep 3
    Sample '1 launch idle, no connection'
    Invoke-El 'New connection'
    Set-El 'Name' 'bench'; Set-El 'Host' $PgHost; Set-El 'Port' "$PgPort"
    Set-El 'Username' $PgUser; Set-El 'Database' $PgDatabase
    Invoke-El 'Save & connect'
    [void](Find-El 'SQL editor' 'Edit'); Start-Sleep 2
    Sample '2 connected, query tab'
    # The editor has keyboard focus inside a window that does not.
    $f0 = Frames; Start-Sleep 10; $timing.queryTabFramesPer10s = (Frames) - $f0
    $timing.windowWasForeground = [Bench]::GetForegroundWindow() -eq $script:hwnd
    Invoke-El 'events' 'TreeItem'
    [void](Find-Match '^Page 1 of'); Start-Sleep 2
    Sample '3 table tab open (events)'
    if (Quiet) {
        $c0 = Cpu-Ms; $x0 = Ctx-Switches; $f0 = Frames; $t = [Diagnostics.Stopwatch]::StartNew()
        Start-Sleep $CpuSeconds
        $timing.idleTableFrames = (Frames) - $f0
        $timing.idleCpuPct = [math]::Round(((Cpu-Ms) - $c0) / $t.Elapsed.TotalMilliseconds * 100, 3)
        $timing.idleCtxPerSec = [math]::Round(((Ctx-Switches) - $x0) / $t.Elapsed.TotalSeconds, 1)
    }
    # Wheel notches over the grid, reversing every 40.
    $gr = (Find-Match '^\d{4}-\d\d-\d\dT' 10).Current.BoundingRectangle
    $sx = [int]($gr.X + 20); $sy = [int]($gr.Y + 200)
    $busy = -not (Quiet)
    $f0 = Frames; $c0 = Cpu-Ms; $x0 = Ctx-Switches; $t = [Diagnostics.Stopwatch]::StartNew(); $i = 0
    while ($t.Elapsed.TotalSeconds -lt $ScrollSeconds) {
        $d = if ([math]::Floor($i / 40) % 2 -eq 0) { -120 } else { 120 }
        [Bench]::Wheel($script:hwnd, $sx, $sy, $d); $i++; Start-Sleep -Milliseconds 16
    }
    $el = $t.Elapsed.TotalMilliseconds; Start-Sleep -Milliseconds 300
    if (-not ($busy -or (Compiling))) {
        $timing.scrollCpuPct = [math]::Round(((Cpu-Ms) - $c0) / $el * 100, 1)
        $timing.scrollFps = [math]::Round(((Frames) - $f0) / (($el + 300) / 1000), 1)
        $timing.scrollCtxPerSec = [math]::Round(((Ctx-Switches) - $x0) / ($el / 1000), 1)
    }
    $timing.wheelNotches = $i
    Sample '4 after scrolling the table'
    Invoke-El 'Query 1' 'TabItem'; Start-Sleep 1
    Run-Query 'SELECT * FROM events ORDER BY id LIMIT 10000'
    Sample '5 10,000-row wide result'
    foreach ($q in 'SELECT * FROM events ORDER BY id DESC LIMIT 10000', 'SELECT id, body, body AS body2, payload FROM events WHERE id > 100000 ORDER BY id LIMIT 10000') {
        Invoke-El 'New query'; [void](Find-El 'SQL editor' 'Edit'); Start-Sleep 1
        Run-Query $q
    }
    Invoke-El 'users' 'TreeItem'; Start-Sleep 2
    Invoke-El 'products' 'TreeItem'; Start-Sleep 2
    Sample '6a six tabs open'
    # From here on only the tab strip's and sidebar's subtrees are searched:
    # builds before 2026-11-22 repeated an accessibility id among the
    # window's top-level nodes once the last tab closed, and walking those
    # then never ended. Only the active tab has a close button, so each close
    # brings the next one.
    $tabs = Find-El 'Tabs' 'Tab'
    $actions = Find-El 'Connection actions' 'Button'
    $sidebar = Find-El 'Connections' 'Group'
    $all = [Windows.Automation.Condition]::TrueCondition
    $closers = { @($tabs.FindAll([Windows.Automation.TreeScope]::Descendants, $all) | Where-Object { $_.Current.Name -like 'Close *' }) }
    for ($n = 0; $n -lt 12; $n++) {
        $c = & $closers
        if ($c.Count -eq 0) { break }
        $name = $c[0].Current.Name
        try { $c[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke() } catch { }
        $t = [Diagnostics.Stopwatch]::StartNew()
        while ($t.Elapsed.TotalSeconds -lt 5 -and (& $closers | Where-Object { $_.Current.Name -eq $name })) { Start-Sleep -Milliseconds 200 }
    }
    $timing.tabsLeftAfterClosing = (& $closers).Count
    Start-Sleep 2
    Sample '6b all tabs closed'
    $actions.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke(); Start-Sleep 1
    $cond = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, 'Disconnect')
    $item = $null; $t = [Diagnostics.Stopwatch]::StartNew()
    while (-not $item -and $t.Elapsed.TotalSeconds -lt 10) {
        $item = $sidebar.FindFirst([Windows.Automation.TreeScope]::Descendants, $cond)
        if (-not $item) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $item) { throw 'no Disconnect item' }
    $item.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke(); Start-Sleep 3
    Sample '7 disconnected'
    Start-Sleep $IdleSeconds
    Sample "8 idle $IdleSeconds s after"
    # The monitor's full size without activating (SW_MAXIMIZE would take the
    # foreground): the same swap chain size as maximized.
    $mon = [Windows.Forms.Screen]::FromHandle($script:hwnd).Bounds
    [void][Bench]::SetWindowPos($script:hwnd, [IntPtr]1, $mon.X, $mon.Y, $mon.Width, $mon.Height, 0x10)
    Start-Sleep 3
    Sample "9a full monitor $($mon.Width)x$($mon.Height)"
    [void][Bench]::ShowWindow($script:hwnd, 7)   # SW_SHOWMINNOACTIVE
    Start-Sleep 5
    Sample '9b minimized'
    if (Quiet) {
        $c0 = Cpu-Ms; $x0 = Ctx-Switches; $t = [Diagnostics.Stopwatch]::StartNew(); Start-Sleep 10
        $timing.minimizedCpuPct = [math]::Round(((Cpu-Ms) - $c0) / $t.Elapsed.TotalMilliseconds * 100, 3)
        $timing.minimizedCtxPerSec = [math]::Round(((Ctx-Switches) - $x0) / $t.Elapsed.TotalSeconds, 1)
    }
} finally {
    Stop-Process -Id $script:procId -Force -ErrorAction SilentlyContinue
    $rows | Export-Csv (Join-Path $Out 'states.csv') -NoTypeInformation
    [pscustomobject]$timing | ConvertTo-Json | Set-Content (Join-Path $Out 'timing.json')
}
$rows | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
[pscustomobject]$timing | Format-List | Out-String | Write-Host
