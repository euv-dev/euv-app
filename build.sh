#!/usr/bin/env bash
#
# EUV 跨平台构建脚本
# 用法:
#   ./build.sh                    默认构建 Android release 包
#   ./build.sh android            构建 Android release 包（签名 + 混淆）
#   ./build.sh android debug      构建 Android debug 包
#   ./build.sh ios                构建 iOS release 包
#   ./build.sh ios debug          构建 iOS debug 包
#   ./build.sh all                构建所有平台 release 包
#

set -euo pipefail

PLATFORM="${1:-android}"
MODE="${2:-release}"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }
step()  { echo -e "${BLUE}[STEP]${NC} $*"; }

# 切换到项目根目录
cd "$(dirname "$0")"

# 读取配置
CONFIG_FILE="app.config.json"
if [ ! -f "$CONFIG_FILE" ]; then
    error "未找到 $CONFIG_FILE，请确保配置文件存在"
fi

APP_NAME=$(node -e "console.log(require('./$CONFIG_FILE').app.name)")
APP_VERSION=$(node -e "console.log(require('./$CONFIG_FILE').app.version)")
APP_ID=$(node -e "console.log(require('./$CONFIG_FILE').app.identifier)")

info "应用: $APP_NAME v$APP_VERSION ($APP_ID)"

# 校验参数
case "$PLATFORM" in
    android|ios|all)
        ;;
    release|debug)
        # 兼容旧用法: ./build.sh release 或 ./build.sh debug
        MODE="$PLATFORM"
        PLATFORM="android"
        ;;
    -h|--help|help)
        echo "用法: $0 [platform] [mode]"
        echo ""
        echo "平台 (platform):"
        echo "  android     构建 Android APK（默认）"
        echo "  ios         构建 iOS IPA"
        echo "  all         构建所有平台"
        echo ""
        echo "模式 (mode):"
        echo "  release     签名的 release 包（默认）"
        echo "  debug       debug 包"
        exit 0
        ;;
    *)
        error "未知平台: $PLATFORM（仅支持 android、ios、all）"
        ;;
esac

case "$MODE" in
    release|debug)
        ;;
    *)
        error "未知模式: $MODE（仅支持 release 或 debug）"
        ;;
esac

# 检查依赖
command -v cargo >/dev/null 2>&1 || error "未找到 cargo，请先安装 Rust 工具链"

# 使用 Node.js 20+
if [ -f "$HOME/.nvm/nvm.sh" ]; then
    source "$HOME/.nvm/nvm.sh"
    nvm use 20 --silent 2>/dev/null || nvm use node --silent
fi
command -v npx >/dev/null 2>&1 || error "未找到 npx，请先安装 Node.js"

# Step 1: 应用配置
step "应用配置到各平台文件..."
node scripts/apply-config.js

# ===== 构建函数 =====

build_android() {
    local mode="$1"
    info "开始构建 Android $mode 包..."

    # 使用 Java 17（Kotlin/AGP 不兼容 Java 25）
    export JAVA_HOME="/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home"
    export PATH="$JAVA_HOME/bin:$PATH"

    info "Node: $(node --version), Java: $(java -version 2>&1 | head -1)"

    BUILD_START=$(date +%s)

    # 备份自定义的 RustWebViewClient.kt（tauri CLI 会覆盖 generated/ 目录）
    GENERATED_DIR="src-tauri/gen/android/app/src/main/java/com/euv/generated"
    BACKUP_FILE="/tmp/euv_RustWebViewClient_backup.kt"
    cp "$GENERATED_DIR/RustWebViewClient.kt" "$BACKUP_FILE"

    # 运行 tauri CLI 编译 Rust + 生成 Kotlin 文件（Gradle 构建会失败，忽略）
    if [ "$mode" = "release" ]; then
        npx @tauri-apps/cli android build --apk 2>&1 || true
    else
        npx @tauri-apps/cli android build --apk --debug 2>&1 || true
    fi

    # 恢复自定义的 RustWebViewClient.kt
    cp "$BACKUP_FILE" "$GENERATED_DIR/RustWebViewClient.kt"
    info "已恢复自定义 RustWebViewClient.kt"

    # 清除 Kotlin 编译缓存
    rm -rf "src-tauri/gen/android/app/build/tmp/kotlin-classes"

    # 执行 Gradle 构建
    info "执行 Gradle 构建..."
    ANDROID_DIR="src-tauri/gen/android"
    if [ "$mode" = "release" ]; then
        "$ANDROID_DIR/gradlew" --project-dir "$ANDROID_DIR" assembleUniversalRelease
    else
        "$ANDROID_DIR/gradlew" --project-dir "$ANDROID_DIR" assembleUniversalDebug
    fi

    BUILD_END=$(date +%s)
    BUILD_DURATION=$((BUILD_END - BUILD_START))

    # 定位产物
    APK_DIR="src-tauri/gen/android/app/build/outputs/apk"
    if [ "$mode" = "release" ]; then
        APK_PATH="$APK_DIR/universal/release/app-universal-release.apk"
        OUTPUT_NAME="${APP_NAME,,}.apk"
    else
        APK_PATH="$APK_DIR/universal/debug/app-universal-debug.apk"
        OUTPUT_NAME="${APP_NAME,,}-debug.apk"
    fi

    if [ -f "$APK_PATH" ]; then
        cp "$APK_PATH" "$OUTPUT_NAME"
        APK_SIZE=$(du -h "$OUTPUT_NAME" | cut -f1)
        info "Android 构建完成! 耗时 ${BUILD_DURATION}s"
        info "产物路径: ./$OUTPUT_NAME"
        info "文件大小: $APK_SIZE"

        # 自动安装到已连接的设备
        if command -v adb >/dev/null 2>&1; then
            DEVICE_COUNT=$(adb devices | grep -c -w 'device' || true)
            if [ "$DEVICE_COUNT" -gt 0 ]; then
                info "检测到设备，正在安装..."
                adb install -r "$OUTPUT_NAME"
                info "安装完成!"
            else
                warn "未检测到已连接的设备，跳过安装"
            fi
        else
            warn "未找到 adb，跳过自动安装"
        fi
    else
        warn "构建可能完成，但未在预期路径找到 APK"
        find "$APK_DIR" -name "*.apk" 2>/dev/null | while read -r apk; do
            info "  找到: $apk"
        done
    fi
}

build_ios() {
    local mode="$1"
    info "开始构建 iOS $mode 包..."

    # 检查 macOS 环境
    if [[ "$(uname)" != "Darwin" ]]; then
        error "iOS 构建仅支持 macOS"
    fi

    # 检查 Xcode
    command -v xcodebuild >/dev/null 2>&1 || error "未找到 xcodebuild，请先安装 Xcode"

    BUILD_START=$(date +%s)

    # 初始化 iOS 项目（如果尚未初始化）
    if [ ! -d "src-tauri/gen/apple" ]; then
        info "初始化 iOS 项目..."
        npx @tauri-apps/cli ios init
    fi

    # 运行 tauri iOS 构建
    if [ "$mode" = "release" ]; then
        npx @tauri-apps/cli ios build
    else
        npx @tauri-apps/cli ios build --debug
    fi

    BUILD_END=$(date +%s)
    BUILD_DURATION=$((BUILD_END - BUILD_START))

    info "iOS 构建完成! 耗时 ${BUILD_DURATION}s"

    # 查找产物
    IPA_DIR="src-tauri/gen/apple/build"
    if [ -d "$IPA_DIR" ]; then
        find "$IPA_DIR" -name "*.ipa" -o -name "*.app" 2>/dev/null | while read -r artifact; do
            info "  产物: $artifact"
        done
    fi
}

# ===== 执行构建 =====

info "构建平台: $PLATFORM, 模式: $MODE"
echo ""

case "$PLATFORM" in
    android)
        build_android "$MODE"
        ;;
    ios)
        build_ios "$MODE"
        ;;
    all)
        step "===== 构建 Android ====="
        build_android "$MODE"
        echo ""
        step "===== 构建 iOS ====="
        build_ios "$MODE"
        ;;
esac

echo ""
info "全部构建完成!"
