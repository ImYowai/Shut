Add-Type -AssemblyName System.Drawing
$src = [System.Drawing.Bitmap]::FromFile('C:\Users\yamau\Desktop\Shut\docs\verification\shut-remote-session.png')
$h = 200
$bmp = New-Object System.Drawing.Bitmap $src.Width, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.DrawImage($src, (New-Object System.Drawing.Rectangle 0, 0, $src.Width, $h), (New-Object System.Drawing.Rectangle 0, ($src.Height - $h), $src.Width, $h), [System.Drawing.GraphicsUnit]::Pixel)
$bmp.Save('C:\Users\yamau\Desktop\Shut\docs\verification\shut-remote-bottom.png')
Write-Output "cropped bottom $($src.Width)x$h"
