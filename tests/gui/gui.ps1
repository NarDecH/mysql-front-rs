param(
    [Parameter(Mandatory = $true)][string]$Cmd,
    [string]$Arg1,
    [string]$Arg2
)

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
[Win]::SetProcessDPIAware() | Out-Null

function Get-AppWindow {
    $p = Get-Process mysql-front-rs -ErrorAction Stop |
        Where-Object { $_.MainWindowHandle -ne 0 } |
        Select-Object -First 1
    if (-not $p) { throw "window not found (process has no main window)" }
    return $p.MainWindowHandle
}

function Show-Window {
    $h = Get-AppWindow
    [Win]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 250
    $r = New-Object Win+RECT
    [Win]::GetWindowRect($h, [ref]$r) | Out-Null
    "$($r.L),$($r.T),$($r.R),$($r.B)"
}

switch ($Cmd) {
    "rect" {
        Show-Window
    }
    "snap" {
        Show-Window | Out-Null   # bring to front first
        $bmp = New-Object System.Drawing.Bitmap([System.Windows.Forms.Screen]::PrimaryScreen.Bounds.Width, [System.Windows.Forms.Screen]::PrimaryScreen.Bounds.Height)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
        $g.Dispose()
        $bmp.Save($Arg1, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        Write-Output "saved $Arg1"
    }
    "click" {
        Show-Window | Out-Null
        $x = [int]$Arg1; $y = [int]$Arg2
        [Win]::SetCursorPos($x, $y) | Out-Null
        Start-Sleep -Milliseconds 120
        [Win]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)  # down
        [Win]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)  # up
        Start-Sleep -Milliseconds 350
        Write-Output "clicked $x $y"
    }
    "type" {
        Show-Window | Out-Null
        [System.Windows.Forms.SendKeys]::SendWait($Arg1)
        Start-Sleep -Milliseconds 250
        Write-Output "typed"
    }
    "key" {
        Show-Window | Out-Null
        [System.Windows.Forms.SendKeys]::SendWait($Arg1)
        Start-Sleep -Milliseconds 350
        Write-Output "key sent"
    }
}
