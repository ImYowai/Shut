param([string]$OutFile = "region.png", [int]$X = 0, [int]$Y = -1080, [int]$W = 1920, [int]$H = 1080)
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap $W, $H
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($X, $Y, 0, 0, $bmp.Size)
$bmp.Save($OutFile, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Output "saved $W x $H at $X,$Y -> $OutFile"
