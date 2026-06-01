#!/usr/bin/env bash
#
# EUV Android Build Script
# Usage:
#   ./build.sh                    Build Android release package (default)
#   ./build.sh android            Build Android release package
#   ./build.sh android debug      Build Android debug package
#   ./build.sh release            Build Android release package
#   ./build.sh debug              Build Android debug package
#

set -euo pipefail

PLATFORM="${1:-android}"
MODE="${2:-release}"

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }
step()  { echo -e "${BLUE}[STEP]${NC} $*"; }

# Switch to project root
cd "$(dirname "$0")"

# Read config
CONFIG_FILE="app.config.json"
if [ ! -f "$CONFIG_FILE" ]; then
    error "Config file not found: $CONFIG_FILE"
fi

APP_NAME=$(node -e "console.log(require('./$CONFIG_FILE').app.name)")
APP_VERSION=$(node -e "console.log(require('./$CONFIG_FILE').app.version)")
APP_ID=$(node -e "console.log(require('./$CONFIG_FILE').app.identifier)")

info "App: $APP_NAME v$APP_VERSION ($APP_ID)"

# Validate parameters
case "$PLATFORM" in
    android)
        ;;
    release|debug)
        MODE="$PLATFORM"
        PLATFORM="android"
        ;;
    -h|--help|help)
        echo "Usage: $0 [platform] [mode]"
        echo ""
        echo "Platform:"
        echo "  android     Build Android APK (default)"
        echo ""
        echo "Mode:"
        echo "  release     Signed release package (default)"
        echo "  debug       Debug package"
        exit 0
        ;;
    *)
        error "Unknown platform: $PLATFORM (only android)"
        ;;
esac

case "$MODE" in
    release|debug)
        ;;
    *)
        error "Unknown mode: $MODE (only release or debug)"
        ;;
esac

# Check dependencies
command -v cargo >/dev/null 2>&1 || error "cargo not found, please install Rust toolchain"

# Use Node.js 20+
if [ -f "$HOME/.nvm/nvm.sh" ]; then
    source "$HOME/.nvm/nvm.sh"
    nvm use 20 --silent 2>/dev/null || nvm use node --silent
fi
command -v npx >/dev/null 2>&1 || error "npx not found, please install Node.js"

# Step 1: Apply config
step "Applying config to platform files..."
node scripts/apply-config.js

# ===== Build =====

info "Starting Android $MODE build..."

# Use Java 17 (Kotlin/AGP incompatible with Java 25)
export JAVA_HOME="/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home"
export PATH="$JAVA_HOME/bin:$PATH"

# Android SDK / NDK
export ANDROID_HOME="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
if [ -z "${NDK_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
    NDK_VER=$(ls "$ANDROID_HOME/ndk" 2>/dev/null | sort -V | tail -1)
    export NDK_HOME="$ANDROID_HOME/ndk/$NDK_VER"
fi
[ -n "${NDK_HOME:-}" ] || error "Android NDK not found, install via sdkmanager"
info "NDK_HOME: $NDK_HOME"

info "Node: $(node --version), Java: $(java -version 2>&1 | head -1)"

BUILD_START=$(date +%s)

# Backup custom RustWebViewClient.kt (tauri CLI overwrites generated/ directory)
GENERATED_DIR="src-tauri/gen/android/app/src/main/java/com/euv/generated"
BACKUP_FILE="/tmp/euv_RustWebViewClient_backup.kt"
cp "$GENERATED_DIR/RustWebViewClient.kt" "$BACKUP_FILE"

# Run tauri CLI to compile Rust + generate Kotlin files (Gradle build will fail, ignore)
if [ "$MODE" = "release" ]; then
    npx @tauri-apps/cli android build --apk 2>&1 || true
else
    npx @tauri-apps/cli android build --apk --debug 2>&1 || true
fi

# Restore custom RustWebViewClient.kt
cp "$BACKUP_FILE" "$GENERATED_DIR/RustWebViewClient.kt"
info "Restored custom RustWebViewClient.kt"

# Copy .so files to jniLibs for all architectures
# Kotlin Rust.kt calls System.loadLibrary("euv_lib") which expects libeuv_lib.so
LIB_NAME="libeuv_lib.so"
JNI_BASE_DIR="src-tauri/gen/android/app/src/main/jniLibs"
declare -A ARCH_MAP=(
    ["arm64-v8a"]="aarch64-linux-android"
    ["armeabi-v7a"]="armv7-linux-androideabi"
    ["x86"]="i686-linux-android"
    ["x86_64"]="x86_64-linux-android"
)
for ARCH in "${!ARCH_MAP[@]}"; do
    RUST_TARGET="${ARCH_MAP[$ARCH]}"
    JNI_DIR="$JNI_BASE_DIR/$ARCH"
    TARGET_SO="$JNI_DIR/$LIB_NAME"
    RUST_SO="src-tauri/target/$RUST_TARGET/$MODE/libeuv_lib.so"
    rm -f "$TARGET_SO"
    if [ -f "$RUST_SO" ]; then
        cp "$RUST_SO" "$TARGET_SO"
        info "Copied $ARCH $LIB_NAME from Rust target"
    else
        warn "$ARCH .so file not found at: $RUST_SO — APK will be missing native lib for $ARCH"
    fi
done

# Run Gradle build
info "Running Gradle build..."
ANDROID_DIR="src-tauri/gen/android"
if [ "$MODE" = "release" ]; then
    "$ANDROID_DIR/gradlew" --project-dir "$ANDROID_DIR" assembleUniversalRelease
else
    "$ANDROID_DIR/gradlew" --project-dir "$ANDROID_DIR" assembleUniversalDebug
fi

BUILD_END=$(date +%s)
BUILD_DURATION=$((BUILD_END - BUILD_START))

# Locate output
APK_DIR="src-tauri/gen/android/app/build/outputs/apk"
local app_lower
app_lower=$(printf '%s' "$APP_NAME" | tr '[:upper:]' '[:lower:]')
if [ "$MODE" = "release" ]; then
    APK_PATH="$APK_DIR/universal/release/app-universal-release.apk"
    OUTPUT_NAME="${app_lower}.apk"
else
    APK_PATH="$APK_DIR/universal/debug/app-universal-debug.apk"
    OUTPUT_NAME="${app_lower}-debug.apk"
fi

if [ -f "$APK_PATH" ]; then
    cp "$APK_PATH" "$OUTPUT_NAME"
    APK_SIZE=$(du -h "$OUTPUT_NAME" | cut -f1)
    info "Android build complete! Duration: ${BUILD_DURATION}s"
    info "Output: ./$OUTPUT_NAME"
    info "Size: $APK_SIZE"

    # Auto-install to connected device
    if command -v adb >/dev/null 2>&1; then
        DEVICE_COUNT=$(adb devices | grep -c -w 'device' || true)
        if [ "$DEVICE_COUNT" -gt 0 ]; then
            info "Device detected, installing..."
            adb install -r "$OUTPUT_NAME"
            info "Install complete!"
        else
            warn "No connected device detected, skipping install"
        fi
    else
        warn "adb not found, skipping auto-install"
    fi
else
    warn "Build may have completed, but APK not found at expected path"
    find "$APK_DIR" -name "*.apk" 2>/dev/null | while read -r apk; do
        info "  Found: $apk"
    done
fi

echo ""
info "Build complete!"
