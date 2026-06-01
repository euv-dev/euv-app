#!/usr/bin/env bash
#
# EUV Android 构建脚本
# 用法:
#   ./build.sh            默认构建 release 包（签名 + 混淆）
#   ./build.sh release    构建 release 包（签名 + 混淆）
#   ./build.sh debug      构建 debug 包
#

set -euo pipefail

MODE="${1:-release}"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# 切换到项目根目录
cd "$(dirname "$0")"

# 校验构建模式
case "$MODE" in
    release|debug)
        info "构建模式: $MODE"
        ;;
    -h|--help|help)
        echo "用法: $0 [release|debug]"
        echo ""
        echo "  release   构建签名的 release APK（默认）"
        echo "  debug     构建 debug APK"
        exit 0
        ;;
    *)
        error "未知模式: $MODE（仅支持 release 或 debug）"
        ;;
esac

# 检查依赖
command -v cargo >/dev/null 2>&1 || error "未找到 cargo，请先安装 Rust 工具链"

# 使用 Node.js 20+（tauri CLI 需要 node:fs 等模块）
if [ -f "$HOME/.nvm/nvm.sh" ]; then
    source "$HOME/.nvm/nvm.sh"
    nvm use 20 --silent 2>/dev/null || nvm use node --silent
fi
command -v npx >/dev/null 2>&1 || error "未找到 npx，请先安装 Node.js"

# 使用 Java 17（Kotlin/AGP 不兼容 Java 25）
export JAVA_HOME="/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home"
export PATH="$JAVA_HOME/bin:$PATH"

# 构建
info "开始构建 Android $MODE 包..."
info "Node: $(node --version), Java: $(java -version 2>&1 | head -1)"

BUILD_START=$(date +%s)

# 备份自定义的 RustWebViewClient.kt（tauri CLI 会覆盖 generated/ 目录）
GENERATED_DIR="src-tauri/gen/android/app/src/main/java/com/euv/generated"
BACKUP_FILE="/tmp/euv_RustWebViewClient_backup.kt"
cp "$GENERATED_DIR/RustWebViewClient.kt" "$BACKUP_FILE"

# 运行 tauri CLI 编译 Rust + 生成 Kotlin 文件（Gradle 构建会失败，忽略）
if [ "$MODE" = "release" ]; then
    npx @tauri-apps/cli android build --apk 2>&1 || true
else
    npx @tauri-apps/cli android build --apk --debug 2>&1 || true
fi

# 恢复自定义的 RustWebViewClient.kt（覆盖 tauri CLI 生成的默认版本）
cp "$BACKUP_FILE" "$GENERATED_DIR/RustWebViewClient.kt"
info "已恢复自定义 RustWebViewClient.kt"

# 清除 Kotlin 编译缓存，确保使用恢复后的源码
rm -rf "src-tauri/gen/android/app/build/tmp/kotlin-classes"

# 执行 Gradle 构建
info "执行 Gradle 构建..."
ANDROID_DIR="src-tauri/gen/android"
if [ "$MODE" = "release" ]; then
    "$ANDROID_DIR/gradlew" --project-dir "$ANDROID_DIR" assembleUniversalRelease
else
    "$ANDROID_DIR/gradlew" --project-dir "$ANDROID_DIR" assembleUniversalDebug
fi

BUILD_END=$(date +%s)
BUILD_DURATION=$((BUILD_END - BUILD_START))

# 定位产物
APK_DIR="src-tauri/gen/android/app/build/outputs/apk"
if [ "$MODE" = "release" ]; then
    APK_PATH="$APK_DIR/universal/release/app-universal-release.apk"
else
    APK_PATH="$APK_DIR/universal/debug/app-universal-debug.apk"
fi

if [ -f "$APK_PATH" ]; then
    # 复制 APK 到项目根目录
    if [ "$MODE" = "release" ]; then
        OUTPUT_NAME="euv.apk"
    else
        OUTPUT_NAME="euv-debug.apk"
    fi
    cp "$APK_PATH" "$OUTPUT_NAME"
    APK_SIZE=$(du -h "$OUTPUT_NAME" | cut -f1)
    info "构建完成! 耗时 ${BUILD_DURATION}s"
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
    warn "请检查: $APK_DIR/"
    # 尝试列出实际产物
    find "$APK_DIR" -name "*.apk" 2>/dev/null | while read -r apk; do
        info "  找到: $apk"
    done
fi
