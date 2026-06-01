#!/usr/bin/env bash
#
# EUV Android 构建脚本
# 用法:
#   ./build.sh release    构建 release 包（签名 + 混淆）
#   ./build.sh debug      构建 debug 包
#   ./build.sh            默认构建 debug 包
#

set -euo pipefail

MODE="${1:-debug}"

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
        echo "  release   构建签名的 release APK（启用混淆）"
        echo "  debug     构建 debug APK（默认）"
        exit 0
        ;;
    *)
        error "未知模式: $MODE（仅支持 release 或 debug）"
        ;;
esac

# 检查依赖
command -v cargo >/dev/null 2>&1 || error "未找到 cargo，请先安装 Rust 工具链"
command -v npx   >/dev/null 2>&1 || error "未找到 npx，请先安装 Node.js"

# 构建
info "开始构建 Android $MODE 包..."

BUILD_START=$(date +%s)

if [ "$MODE" = "release" ]; then
    npx tauri android build --apk
else
    npx tauri android build --apk --debug
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
else
    warn "构建可能完成，但未在预期路径找到 APK"
    warn "请检查: $APK_DIR/"
    # 尝试列出实际产物
    find "$APK_DIR" -name "*.apk" 2>/dev/null | while read -r apk; do
        info "  找到: $apk"
    done
fi
