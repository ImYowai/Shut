# Launch unified Shut on the SUB monitor (verification only).
param(
    [string]$Url = "",
    [string]$Password = "",
    [switch]$AsHost,
    [switch]$Multi,
    [string]$Exe = "C:\Users\yamau\Desktop\Shut\target\debug\shut.exe"
)

Add-Type -AssemblyName System.Windows.Forms
$scr = [System.Windows.Forms.Screen]::AllScreens | Where-Object { -not $_.Primary } | Select-Object -First 1
if (-not $scr) { Write-Output "no sub monitor"; exit 1 }
$b = $scr.Bounds
$env:SHUT_WINDOW_POS = "$($b.X + 60),$($b.Y + 40),1160,760"
if ($Multi) { $env:SHUT_MULTI = "1" }
if ($Url) { $env:SHUT_REMOTE_URL = $Url }
if ($Password) { $env:SHUT_REMOTE_PASSWORD = $Password }
if ($AsHost) { Start-Process -FilePath $Exe -ArgumentList "--host" }
else { Start-Process -FilePath $Exe }
Write-Output "launched on sub at $($env:SHUT_WINDOW_POS)"
