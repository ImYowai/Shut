Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
$v = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bmp = New-Object System.Drawing.Bitmap $v.Width, $v.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($v.Left, $v.Top, 0, 0, $bmp.Size)
$bmp.Save('C:\Users\yamau\Desktop\Shut\docs\verification\screen.png')
Write-Output "saved $($v.Width)x$($v.Height)"
