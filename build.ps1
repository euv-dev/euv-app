# EUV Android Build Script (Windows)
# Usage:
#   .\build.ps1                    Build Android release package (default)
#   .\build.ps1 android            Build Android release package
#   .\build.ps1 android debug      Build Android debug package
#   .\build.ps1 release            Build Android release package
#   .\build.ps1 debug              Build Android debug package

$ErrorActionPreference = "Stop"

$Platform = if ($args[0]) { $args[0].ToLower() } else { "android" }
$Mode = if ($args[1]) { $args[1].ToLower() } else { "release" }

# Color output
function Write-Info($msg)  { Write-Host -ForegroundColor Green  "[INFO] $msg" }
function Write-Warn($msg)  { Write-Host -ForegroundColor Yellow "[WARN] $msg" }
function Write-Err($msg)   { Write-Host -ForegroundColor Red    "[ERROR] $msg"; exit 1 }
function Write-Step($msg)  { Write-Host -ForegroundColor Blue   "[STEP] $msg" }

# Switch to project root
$projectDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $projectDir

# Read config via node (avoids PowerShell UTF-8 encoding issues)
$configFile = Join-Path $projectDir "app.config.json"
if (-not (Test-Path $configFile)) {
    Write-Err "app.config.json not found at: $configFile"
}

$appName = node -e "console.log(require('./app.config.json').app.name)"
$appVersion = node -e "console.log(require('./app.config.json').app.version)"
$appId = node -e "console.log(require('./app.config.json').app.identifier)"
$appNameLower = $appName.ToLower()

Write-Info "App: $appName v$appVersion ($appId)"

# Validate parameters
switch ($Platform) {
    { $_ -in "android" } { }
    { $_ -in "release", "debug" } {
        $Mode = $Platform
        $Platform = "android"
    }
    { $_ -in "-h", "--help", "help" } {
        Write-Output "Usage: .\build.ps1 [platform] [mode]"
        Write-Output ""
        Write-Output "Platform:"
        Write-Output "  android     Build Android APK (default)"
        Write-Output ""
        Write-Output "Mode:"
        Write-Output "  release     Signed release package (default)"
        Write-Output "  debug       Debug package"
        exit 0
    }
    default {
        Write-Err "Unknown platform: $Platform (only android)"
    }
}

switch ($Mode) {
    { $_ -in "release", "debug" } { }
    default {
        Write-Err "Unknown mode: $Mode (only release or debug)"
    }
}

# Check dependencies
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Err "cargo not found, please install Rust toolchain first"
}

if (-not (Get-Command npx -ErrorAction SilentlyContinue)) {
    Write-Err "npx not found, please install Node.js first"
}

# Step 1: Apply config
Write-Step "Applying config to platform files..."
node scripts/apply-config.js

# ===== Build =====

Write-Info "Starting Android $Mode build..."

# Environment
$env:ANDROID_HOME = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { "$env:LOCALAPPDATA\Android\Sdk" }
$env:ANDROID_SDK_ROOT = $env:ANDROID_HOME

if (-not $env:NDK_HOME) {
    $ndkDir = Join-Path $env:ANDROID_HOME "ndk"
    if (Test-Path $ndkDir) {
        $ndkVer = Get-ChildItem $ndkDir -Directory | Sort-Object Name -Descending | Select-Object -First 1
        if ($ndkVer) {
            $env:NDK_HOME = $ndkVer.FullName
        }
    }
}
if (-not $env:NDK_HOME -or -not (Test-Path $env:NDK_HOME)) {
    Write-Err "Android NDK not found, install via sdkmanager"
}
Write-Info "NDK_HOME: $env:NDK_HOME"

$nodeVersion = node --version
$prevErrorAction = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$javaVersion = java -version 2>&1 | Select-Object -First 1
$ErrorActionPreference = $prevErrorAction
Write-Info "Node: $nodeVersion, Java: $javaVersion"

$buildStart = Get-Date

# Backup custom RustWebViewClient.kt (tauri CLI overwrites generated/ directory)
$generatedDir = Join-Path $projectDir "src-tauri\gen\android\app\src\main\java\com\euv\generated"
$backupFile = Join-Path $env:TEMP "euv_RustWebViewClient_backup.kt"
$clientFile = Join-Path $generatedDir "RustWebViewClient.kt"
if (Test-Path $clientFile) {
    Copy-Item $clientFile $backupFile -Force
    Write-Info "Backed up RustWebViewClient.kt"
}

# Run tauri CLI to compile Rust + generate Kotlin files
# Gradle build will fail due to identifier mismatch (com.yudong.euv vs com.euv),
# but Rust compilation and Kotlin generation still succeed — ignore the error.
Write-Step "Running tauri CLI to compile Rust (Gradle will fail, expected)..."
$prevErrorAction = $ErrorActionPreference
$ErrorActionPreference = "Continue"
if ($Mode -eq "release") {
    $tauriOutput = npx @tauri-apps/cli@latest android build --apk 2>&1
} else {
    $tauriOutput = npx @tauri-apps/cli@latest android build --apk --debug 2>&1
}
$ErrorActionPreference = $prevErrorAction
$tauriOutput | ForEach-Object { $_.ToString() } | Write-Output
if ($LASTEXITCODE -ne 0) {
    Write-Warn "tauri CLI exited with code $LASTEXITCODE (Gradle failure expected, Rust compilation succeeded)"
}

# Restore custom RustWebViewClient.kt
if (Test-Path $backupFile) {
    Copy-Item $backupFile $clientFile -Force
    Write-Info "Restored custom RustWebViewClient.kt"
}

# Fix build.gradle.kts
$gradleFile = Join-Path $projectDir "src-tauri\gen\android\app\build.gradle.kts"
if (Test-Path $gradleFile) {
    $gradleContent = Get-Content $gradleFile -Raw
    $gradleContent = $gradleContent -replace 'truri-app', 'euv-app'
    $gradleContent = $gradleContent -replace '\bmanifestPlaceholders\["usesCleartextTraffic"\] = "false"', 'manifestPlaceholders["usesCleartextTraffic"] = "true"'
    $gradleContent | Set-Content $gradleFile -Encoding UTF8 -Force
    Write-Info "Fixed keystore path and usesCleartextTraffic in build.gradle.kts"
}

# Clear Kotlin incremental cache to avoid cross-drive relative path errors
$kotlinCacheDir = Join-Path $projectDir "src-tauri\gen\android\app\build\tmp\kotlin-classes"
if (Test-Path $kotlinCacheDir) {
    try {
        Remove-Item $kotlinCacheDir -Recurse -Force -ErrorAction Stop
        Write-Info "Cleared Kotlin incremental cache"
    } catch {
        Write-Info "Kotlin cache partially locked, skipping cleanup"
    }
}
$buildSrcCacheDir = Join-Path $projectDir "src-tauri\gen\android\buildSrc\build"
if (Test-Path $buildSrcCacheDir) {
    try {
        Remove-Item $buildSrcCacheDir -Recurse -Force -ErrorAction Stop
        Write-Info "Cleared buildSrc cache"
    } catch {
        Write-Info "buildSrc cache partially locked, skipping cleanup"
    }
}

# Clear all build caches
$buildDir = Join-Path $projectDir "src-tauri\gen\android\app\build"
if (Test-Path $buildDir) {
    try {
        Remove-Item $buildDir -Recurse -Force -ErrorAction Stop
        Write-Info "Cleared Android build cache"
    } catch {
        Write-Info "Android build cache partially locked, skipping cleanup"
    }
}

# Copy .so files to jniLibs for all architectures
# Kotlin Rust.kt calls System.loadLibrary("euv_lib") which expects libeuv_lib.so
$libName = "libeuv_lib.so"
$jniBaseDir = Join-Path $projectDir "src-tauri\gen\android\app\src\main\jniLibs"
$archMap = @{
    "arm64-v8a"      = "aarch64-linux-android"
    "armeabi-v7a"    = "armv7-linux-androideabi"
    "x86"            = "i686-linux-android"
    "x86_64"         = "x86_64-linux-android"
}
foreach ($arch in $archMap.Keys) {
    $rustTarget = $archMap[$arch]
    $jniDir = Join-Path $jniBaseDir $arch
    $targetSo = Join-Path $jniDir $libName
    # Rust [lib] name = "euv_lib" produces libeuv_lib.so
    $rustReleaseSo = Join-Path $projectDir "src-tauri\target\$rustTarget\release\libeuv_lib.so"
    $rustDebugSo = Join-Path $projectDir "src-tauri\target\$rustTarget\debug\libeuv_lib.so"
    $rustSo = if ($Mode -eq "release") { $rustReleaseSo } else { $rustDebugSo }
    # Remove old file if present
    if (Test-Path $targetSo) {
        Remove-Item $targetSo -Force -ErrorAction SilentlyContinue
    }
    # Copy .so from Rust target to jniLibs
    if (Test-Path $rustSo) {
        Copy-Item $rustSo $targetSo -Force
        Write-Info "Copied $arch $libName from Rust target"
    } else {
        Write-Warn "$arch .so file not found at: $rustSo — APK will be missing native lib for $arch"
    }
}

# Run Gradle build
Write-Step "Running Gradle build..."
$androidDir = Join-Path $projectDir "src-tauri\gen\android"
Set-Location $androidDir
if ($Mode -eq "release") {
    .\gradlew assembleUniversalRelease --no-daemon
} else {
    .\gradlew assembleUniversalDebug --no-daemon
}

$buildEnd = Get-Date
$buildDuration = [math]::Round(($buildEnd - $buildStart).TotalSeconds)

# Locate output
$apkDir = Join-Path $androidDir "app\build\outputs\apk"
if ($Mode -eq "release") {
    $apkPath = Join-Path $apkDir "universal\release\app-universal-release.apk"
    $outputName = "$appNameLower.apk"
} else {
    $apkPath = Join-Path $apkDir "universal\debug\app-universal-debug.apk"
    $outputName = "$appNameLower-debug.apk"
}

Set-Location $projectDir

if (Test-Path $apkPath) {
    Copy-Item $apkPath $outputName -Force
    $apkSize = (Get-Item $outputName).Length / 1MB
    $apkSizeStr = "{0:N1} MB" -f $apkSize
    Write-Info "Android build complete! Duration: ${buildDuration}s"
    Write-Info "Output: .\$outputName"
    Write-Info "Size: $apkSizeStr"

    # Auto-install to connected device
    $adbPath = Get-Command adb -ErrorAction SilentlyContinue
    if ($adbPath) {
        $devices = adb devices 2>$null | Select-String -Pattern "\bdevice\b"
        if ($devices) {
            Write-Info "Device detected, installing..."
            adb install -r $outputName
            Write-Info "Install complete!"
        } else {
            Write-Warn "No connected device detected, skipping install"
        }
    } else {
        Write-Warn "adb not found, skipping auto-install"
    }
} else {
    Write-Warn "Build may have completed, but APK not found at expected path"
    Get-ChildItem $apkDir -Filter "*.apk" -Recurse | ForEach-Object {
        Write-Info "  Found: $($_.FullName)"
    }
}

Write-Output ""
Write-Info "Build complete!"
