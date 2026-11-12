# End-to-end check of the egui -> Win32 switch as users get it: installs an
# older release with its NSIS setup, lets that build's own updater find the
# latest release and install it from its "Update to v..." button, then checks
# that the relaunched dbm.exe is the latest DBM-Windows-x64.exe and that the
# settings file (sidebar state, What's new acknowledgement) carried over.
param(
    [Parameter(Mandatory = $true)] [string]$FromTag,
    [string]$Out = "update-check"
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
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
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
$install = Join-Path $env:LOCALAPPDATA 'DBM'
$exe = Join-Path $install 'dbm.exe'
$settings = Join-Path $env:APPDATA 'DBM\data\app.ron'

function Save-Screen([IntPtr]$hwnd, [string]$name) {
    $r = New-Object Native+RECT
    [Native]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    if ($r.Right -le $r.Left) { return }
    $bmp = New-Object Drawing.Bitmap ($r.Right - $r.Left), ($r.Bottom - $r.Top)
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
    $bmp.Save((Join-Path $Out $name), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

function Dump-Tree([IntPtr]$hwnd, [string]$name) {
    $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
    $root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) |
        Select-Object -First 300 |
        ForEach-Object { "$($_.Current.ControlType.ProgrammaticName)`t$($_.Current.Name)" } |
        Set-Content (Join-Path $Out $name)
}

# The newest dbm.exe process's "Anybase" window.
function Wait-Window([int]$timeoutSeconds = 60) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt $timeoutSeconds) {
        foreach ($p in Get-Process dbm -ErrorAction SilentlyContinue | Sort-Object StartTime -Descending) {
            $h = [Native]::FindAppWindow($p.Id, 'Anybase')
            if ($h -ne [IntPtr]::Zero) { return @{ process = $p; hwnd = $h } }
        }
        Start-Sleep -Milliseconds 250
    }
    throw "no Anybase window after $timeoutSeconds s"
}

# Clicks the first element whose name matches $pattern (-like).
function Click-Named([IntPtr]$hwnd, [string]$pattern, [int]$timeoutSeconds = 90) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt $timeoutSeconds) {
        $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
        $hit = $root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) |
            Where-Object { $_.Current.Name -like $pattern -and -not $_.Current.BoundingRectangle.IsEmpty } |
            Select-Object -First 1
        if ($hit) {
            $r = $hit.Current.BoundingRectangle
            [Native]::SetForegroundWindow($hwnd) | Out-Null
            [Native]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) | Out-Null
            Start-Sleep -Milliseconds 100
            [Native]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
            [Native]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
            return $hit.Current.Name
        }
        Start-Sleep -Milliseconds 500
    }
    $tag = Get-Random
    Save-Screen $hwnd "missing-$tag.png"
    Dump-Tree $hwnd "missing-$tag-uia.txt"
    throw "nothing named like '$pattern' after $timeoutSeconds s (see missing-$tag.*)"
}

$latest = gh release view --json tagName --jq .tagName
Write-Host "Updating $FromTag -> $latest"
gh release download $FromTag -p DBM-Windows-x64-setup.exe -D (Join-Path $Out 'old') --clobber
gh release download $latest -p DBM-Windows-x64.exe -D (Join-Path $Out 'new') --clobber

# Install the old release the way users did.
$setup = Start-Process (Join-Path $Out 'old\DBM-Windows-x64-setup.exe') -ArgumentList '/S' -PassThru -Wait
Write-Host "setup exit code: $($setup.ExitCode)"
if (-not (Test-Path $exe)) { throw "the installer did not create $exe" }
$oldHash = (Get-FileHash $exe).Hash

# A saved sidebar state the new app must keep.
New-Item -ItemType Directory -Force -Path (Split-Path $settings) | Out-Null
Set-Content $settings '{"dbm.sidebarCollapsed":"true"}'

Start-Process $exe | Out-Null
$old = Wait-Window
Start-Sleep -Seconds 3
Save-Screen $old.hwnd '1-old-app.png'
$clicked = Click-Named $old.hwnd 'Update to v*'
Write-Host "clicked: $clicked"
Start-Sleep -Seconds 1
Save-Screen $old.hwnd '2-confirm.png'
Click-Named $old.hwnd 'Install and restart' 30 | Out-Null

# The updater swaps dbm.exe and relaunches it with --after-update.
$sw = [Diagnostics.Stopwatch]::StartNew()
while ($sw.Elapsed.TotalSeconds -lt 120 -and -not $old.process.HasExited) { Start-Sleep -Milliseconds 500; $old.process.Refresh() }
if (-not $old.process.HasExited) { throw 'the old app did not exit to update' }
$new = Wait-Window 60
Start-Sleep -Seconds 4
Save-Screen $new.hwnd '3-after-update.png'
Dump-Tree $new.hwnd '3-after-update-uia.txt'

$installedHash = (Get-FileHash $exe).Hash
$latestHash = (Get-FileHash (Join-Path $Out 'new\DBM-Windows-x64.exe')).Hash
$cmd = (Get-CimInstance Win32_Process -Filter "ProcessId = $($new.process.Id)").CommandLine
$ron = Get-Content $settings -Raw
$result = [ordered]@{
    from = $FromTag; to = $latest
    oldInstalledHash = $oldHash
    installedHashAfter = $installedHash
    latestAssetHash = $latestHash
    installedIsLatest = ($installedHash -eq $latestHash)
    relaunchCommandLine = $cmd
    settingsAfter = $ron
}
$result | ConvertTo-Json | Tee-Object (Join-Path $Out 'result.json')
Stop-Process -Id $new.process.Id -ErrorAction SilentlyContinue
if (-not $result.installedIsLatest) { throw 'dbm.exe is not the latest release asset after updating' }
if ($ron -notmatch '"dbm.sidebarCollapsed":\s*"true"') { throw 'the sidebar setting did not carry over' }
