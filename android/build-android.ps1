#requires -Version 5
# NOTE: keep this file ASCII-only. Windows PowerShell 5.1 parses a BOM-less
# UTF-8 file as ANSI, which corrupts multibyte comments and breaks statements.
[CmdletBinding()]
param(
    [ValidateSet('aarch64-linux-android','x86_64-linux-android')]
    [string]$Target = 'aarch64-linux-android',
    [switch]$SignForTesting
)
$ErrorActionPreference = 'Stop'
$projectRoot = $PSScriptRoot
$previousLocation = Get-Location
$previousWrapper = [Environment]::GetEnvironmentVariable('RUSTC_WRAPPER','Process')
$previousTarget = [Environment]::GetEnvironmentVariable('CARGO_TARGET_DIR','Process')
$previousGradle = [Environment]::GetEnvironmentVariable('GRADLE_USER_HOME','Process')
try {
    Set-Location -LiteralPath $projectRoot
    $env:RUSTC_WRAPPER = $null
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target'
    $env:GRADLE_USER_HOME = Join-Path $projectRoot 'target\gradle-home'
    & dx build --platform android --release --target $Target
    if ($LASTEXITCODE -ne 0) { throw "dx build failed ($LASTEXITCODE)" }
    # The dx-generated network_security_config only permits cleartext to
    # 127.0.0.1, so WebView rejects HTTP to LAN / emulator (10.0.2.2) hosts.
    # Replace it with our own config; the actual endpoint restriction lives in
    # the Rust URL validation plus RemoteActivity's origin lock.
    $ourConfig = Join-Path $projectRoot 'config\network-security-config.xml'
    $patchedTargets = Get-ChildItem -LiteralPath (Join-Path $projectRoot 'target\dx') -Directory |
        ForEach-Object { Join-Path $_.FullName 'release\android\app\app\src\main\res\xml\network_security_config.xml' } |
        Where-Object { Test-Path -LiteralPath $_ }
    foreach ($configPath in $patchedTargets) {
        Copy-Item -LiteralPath $ourConfig -Destination $configPath -Force
        Write-Host "Patched network security config: $configPath"
    }
    # dx only produces a debug APK from the generated Gradle project, so run
    # assembleRelease there ourselves for the unsigned release APK. Paths use
    # the fixed dx layout target\dx\<pkg>\release\android\app (no -Recurse:
    # Gradle intermediates contain entries that make recursive enumeration fail).
    $appDir = Get-ChildItem -LiteralPath (Join-Path $projectRoot 'target\dx') -Directory |
        ForEach-Object { Join-Path $_.FullName 'release\android\app' } |
        Where-Object { Test-Path -LiteralPath (Join-Path $_ 'gradlew.bat') } |
        Select-Object -First 1
    if (-not $appDir) { throw 'Generated Gradle project not found under target\dx.' }
    # Remove stale ABI folders from previous builds of a different Target so
    # the packaged APK never ships an outdated libmain.so for another ABI.
    $expectedAbi = if ($Target -eq 'x86_64-linux-android') { 'x86_64' } else { 'arm64-v8a' }
    $jniLibsDir = Join-Path $appDir 'app\src\main\jniLibs'
    if (Test-Path -LiteralPath $jniLibsDir) {
        Get-ChildItem -LiteralPath $jniLibsDir -Directory |
            Where-Object { $_.Name -ne $expectedAbi } |
            ForEach-Object {
                Remove-Item -LiteralPath $_.FullName -Recurse -Force
                Write-Host "Removed stale ABI directory: $($_.FullName)"
            }
    }
    foreach ($cachedDir in @('merged_jni_libs', 'merged_native_libs', 'stripped_native_libs')) {
        $cachedPath = Join-Path $appDir "app\build\intermediates\$cachedDir"
        if (Test-Path -LiteralPath $cachedPath) {
            Remove-Item -LiteralPath $cachedPath -Recurse -Force
        }
    }
    Push-Location -LiteralPath $appDir
    try { & .\gradlew.bat assembleRelease --console=plain -q } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "gradle assembleRelease failed ($LASTEXITCODE)" }
    $apk = Get-Item -LiteralPath (Join-Path $appDir 'app\build\outputs\apk\release\app-release-unsigned.apk') -ErrorAction SilentlyContinue
    if (-not $apk) { throw 'APK output was not found.' }
    if ((Get-Date) - $apk.LastWriteTime -gt (New-TimeSpan -Minutes 30)) {
        throw "APK is stale ($($apk.LastWriteTime)). Gradle packageRelease was not refreshed."
    }
    Write-Host "Release APK: $($apk.FullName)"
    if ($SignForTesting) {
        $sdkRoot = [Environment]::GetEnvironmentVariable('ANDROID_HOME','Process')
        if (-not $sdkRoot) { $sdkRoot = $env:ANDROID_HOME }
        if (-not $sdkRoot) { throw 'ANDROID_HOME is required for APK signing.' }
        $buildTools = Get-ChildItem -LiteralPath (Join-Path $sdkRoot 'build-tools') -Directory |
            Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
        $apksigner = Join-Path $buildTools.FullName 'apksigner.bat'
        $signingDir = Join-Path $projectRoot 'target\signing'
        New-Item -ItemType Directory -Force -Path $signingDir | Out-Null
        $keyPath = Join-Path $signingDir 'shut-development.jks'
        if (-not (Test-Path -LiteralPath $keyPath)) {
            & keytool -genkeypair -keystore $keyPath -storepass android -keypass android -alias shut-development -keyalg RSA -keysize 2048 -validity 3650 -dname 'CN=Shut Development, O=Shut, C=JP'
            if ($LASTEXITCODE -ne 0) { throw 'Development key generation failed.' }
        }
        $artifactDir = Join-Path $projectRoot 'artifacts'
        New-Item -ItemType Directory -Force -Path $artifactDir | Out-Null
        $outputApk = Join-Path $artifactDir "Shut-android-$Target-development.apk"
        & $apksigner sign --ks $keyPath --ks-key-alias shut-development --ks-pass pass:android --key-pass pass:android --out $outputApk $apk.FullName
        if ($LASTEXITCODE -ne 0) { throw 'APK signing failed.' }
        & $apksigner verify --verbose $outputApk
        if ($LASTEXITCODE -ne 0) { throw 'APK signature verification failed.' }
        Get-FileHash -LiteralPath $outputApk -Algorithm SHA256
        Write-Host "Development-signed APK: $outputApk"
    }
} finally {
    [Environment]::SetEnvironmentVariable('RUSTC_WRAPPER',$previousWrapper,'Process')
    [Environment]::SetEnvironmentVariable('CARGO_TARGET_DIR',$previousTarget,'Process')
    [Environment]::SetEnvironmentVariable('GRADLE_USER_HOME',$previousGradle,'Process')
    Set-Location -LiteralPath $previousLocation
}
