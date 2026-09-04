#!/bin/bash

# 仅在系统已安装该 locale 时导出，避免未安装时（如部分 macOS）输出乱码
if locale -a 2>/dev/null | grep -qi 'zh_CN\.utf-8'; then
    export LANG=zh_CN.UTF-8
    export LC_ALL=zh_CN.UTF-8
fi

# ============================================================
# DFY运维平台 启动脚本（适配自 web-craft/start.sh）
# 用途：本地开发 / 构建 / 测试 / 日志查看
# 注意：本项目为 Windows 交付（msi/nsis），此脚本用于开发机（macOS/Linux/WSL）
#       在 Windows 上请使用 WSL 或 Git Bash 运行
# ============================================================

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

APP_NAME="dfy-ops"
APP_DISPLAY_NAME="DFY运维平台"

# ---- 日志配置（统一输出到 debug.log）----
LOG_DIR="$SCRIPT_DIR/logs"
LOG_FILE="$LOG_DIR/debug.log"
DEV_LOG_FILE="$LOG_DIR/debug.log"
VITE_LOG_FILE="$LOG_DIR/debug.log"
TAURI_LOG_FILE="$LOG_DIR/debug.log"
ERROR_LOG_FILE="$LOG_DIR/debug.log"

PID_FILE="$LOG_DIR/$APP_NAME.pid"

# 前端开发端口（Vite，strictPort=true）
VITE_PORT=18720
# 嵌入式 Rust Axum 运维 API 端口（Tauri 进程内绑定）
AXUM_PORT=18721

# 后台 Tauri 进程 PID，用于退出时清理
TAURI_BG=""

# ---- 日志函数 ----
log_info() {
    local msg="[$(date '+%Y-%m-%d %H:%M:%S')] [INFO] $1"
    echo -e "${GREEN}${msg}${NC}"
    echo "$msg" >> "$LOG_FILE" 2>/dev/null
}

log_error() {
    local msg="[$(date '+%Y-%m-%d %H:%M:%S')] [ERROR] $1"
    echo -e "${RED}${msg}${NC}"
    echo "$msg" >> "$LOG_FILE" 2>/dev/null
}

log_warn() {
    local msg="[$(date '+%Y-%m-%d %H:%M:%S')] [WARN] $1"
    echo -e "${YELLOW}${msg}${NC}"
    echo "$msg" >> "$LOG_FILE" 2>/dev/null
}

log_debug() {
    local msg="[$(date '+%Y-%m-%d %H:%M:%S')] [DEBUG] $1"
    echo -e "${CYAN}${msg}${NC}"
    echo "$msg" >> "$DEV_LOG_FILE" 2>/dev/null
}

log_step() {
    echo -e "${BLUE}----------------------------------------${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}----------------------------------------${NC}"
}

# 退出时清理后台 Tauri 进程
cleanup() {
    if [ ! -z "$TAURI_BG" ] && kill -0 "$TAURI_BG" 2>/dev/null; then
        log_info "正在停止 $APP_DISPLAY_NAME 开发进程（PID: $TAURI_BG）..."
        kill -TERM "$TAURI_BG" 2>/dev/null
        sleep 1
        if kill -0 "$TAURI_BG" 2>/dev/null; then
            kill -9 "$TAURI_BG" 2>/dev/null
        fi
    fi
}
trap cleanup EXIT INT TERM

# ---- 环境检查 ----
check_env() {
    log_step "检查开发环境..."

    if ! command -v node &> /dev/null; then
        log_error "Node.js 未安装或不在 PATH 中"
        exit 1
    fi
    log_info "Node.js 版本: $(node --version)"

    if ! command -v pnpm &> /dev/null; then
        log_error "pnpm 未安装或不在 PATH 中"
        log_info "请使用 npm install -g pnpm 安装"
        exit 1
    fi
    log_info "pnpm 版本: $(pnpm --version)"

    if ! command -v cargo &> /dev/null; then
        log_error "Rust/Cargo 未安装或不在 PATH 中"
        log_info "请从 https://rustup.rs 安装 Rust"
        exit 1
    fi
    log_info "Rust 版本: $(rustc --version)"

    if ! command -v tauri &> /dev/null; then
        log_warn "Tauri CLI 未全局安装，将使用项目本地版本（pnpm tauri）"
    fi

    # 端口探测依赖 lsof / curl
    if ! command -v lsof &> /dev/null; then
        log_warn "lsof 未安装，端口释放将不可用（macOS/Linux 通常自带）"
    fi
    if ! command -v curl &> /dev/null; then
        log_warn "curl 未安装，启动就绪检测将降级为仅等待"
    fi

    log_info "环境检查通过"
}

# ---- 目录初始化 ----
create_directories() {
    mkdir -p "$LOG_DIR"
    mkdir -p "$SCRIPT_DIR/.data/dev"

    if [ ! -f "$LOG_FILE" ]; then
        touch "$LOG_FILE"
    fi

    log_debug "日志目录: $LOG_DIR"
    log_debug "数据目录: $SCRIPT_DIR/.data/dev"
}

# ---- 依赖安装 ----
install_deps() {
    log_step "检查项目依赖..."

    cd "$SCRIPT_DIR"

    # 检查 package.json 是否变更，若变更则重新安装依赖
    local PACKAGE_HASH_FILE="$SCRIPT_DIR/.data/dev/.package_hash"
    local CURRENT_HASH
    CURRENT_HASH=$(md5 -q "$SCRIPT_DIR/package.json" 2>/dev/null || md5sum "$SCRIPT_DIR/package.json" 2>/dev/null | awk '{print $1}')

    local NEED_INSTALL=false

    if [ ! -d "node_modules" ]; then
        NEED_INSTALL=true
    elif [ -f "$PACKAGE_HASH_FILE" ] && [ "$CURRENT_HASH" != "$(cat "$PACKAGE_HASH_FILE" 2>/dev/null)" ]; then
        log_info "检测到 package.json 变更，重新安装前端依赖..."
        NEED_INSTALL=true
    fi

    if [ "$NEED_INSTALL" = true ]; then
        log_info "安装前端依赖..."
        pnpm install >> "$DEV_LOG_FILE" 2>&1
        if [ $? -ne 0 ]; then
            log_error "前端依赖安装失败，查看 $DEV_LOG_FILE"
            exit 1
        fi
        echo "$CURRENT_HASH" > "$PACKAGE_HASH_FILE"
        log_info "前端依赖安装完成"
    else
        log_info "前端依赖已存在且未变更"
    fi
}

# ---- 端口释放（占用即杀，确保启动前端口空闲）----
# 用法: kill_port <端口号> [尝试次数]
kill_port() {
    local PORT="$1"
    local MAX_TRIES="${2:-3}"

    for ((try=1; try<=MAX_TRIES; try++)); do
        local PIDS_BY_PORT=""
        if command -v lsof &> /dev/null; then
            PIDS_BY_PORT=$(lsof -tiTCP:"$PORT" -sTCP:LISTEN 2>/dev/null)
        fi
        # 同时按进程名兜底（lsof 在普通用户下可能看不到别的用户进程）
        local PIDS_BY_NAME=""
        if command -v pgrep &> /dev/null; then
            PIDS_BY_NAME=$(pgrep -f "vite|$APP_NAME|tauri" 2>/dev/null | grep -v $$)
        fi

        local PIDS=""
        if [ ! -z "$PIDS_BY_PORT" ]; then
            PIDS="$PIDS $PIDS_BY_PORT"
        fi
        if [ ! -z "$PIDS_BY_NAME" ]; then
            PIDS="$PIDS $PIDS_BY_NAME"
        fi
        PIDS=$(echo "$PIDS" | tr ' ' '\n' | sort -u | grep -v '^$')

        if [ -z "$PIDS" ]; then
            return 0
        fi

        log_warn "端口 $PORT 被占用（PID: $(echo $PIDS | tr '\n' ' ')），正在释放（第 $try 次）..."
        echo "$PIDS" | xargs kill -TERM 2>/dev/null
        sleep 1

        # 复检：仍占用则强制 KILL
        local REMAIN=""
        if command -v lsof &> /dev/null; then
            REMAIN=$(lsof -tiTCP:"$PORT" -sTCP:LISTEN 2>/dev/null)
        fi
        if [ ! -z "$REMAIN" ]; then
            log_warn "端口 $PORT 仍未释放（PID: $REMAIN），强制 KILL -9..."
            echo "$REMAIN" | xargs kill -9 2>/dev/null
            sleep 1
        fi

        # 终检
        local FINAL=""
        if command -v lsof &> /dev/null; then
            FINAL=$(lsof -tiTCP:"$PORT" -sTCP:LISTEN 2>/dev/null)
        fi
        if [ -z "$FINAL" ]; then
            log_info "端口 $PORT 已释放 ✓"
            return 0
        fi
    done

    local STUCK=""
    if command -v lsof &> /dev/null; then
        STUCK=$(lsof -tiTCP:"$PORT" -sTCP:LISTEN 2>/dev/null)
    fi
    if [ ! -z "$STUCK" ]; then
        log_error "端口 $PORT 无法释放（仍被 PID: $STUCK 占用），请手动处理：sudo lsof -tiTCP:$PORT -sTCP:LISTEN | xargs sudo kill -9"
        exit 1
    fi
    return 0
}

# ---- 进程检查与清理 ----
check_and_stop_processes() {
    log_info "检查是否有冲突的进程..."

    APP_PIDS=$(pgrep -f "$APP_NAME" 2>/dev/null | grep -v pgrep 2>/dev/null)
    if [ ! -z "$APP_PIDS" ]; then
        log_warn "发现已运行的 $APP_DISPLAY_NAME 进程:"
        echo "$APP_PIDS" | while read pid; do
            [ -z "$pid" ] && continue
            echo "  PID: $pid, 命令: $(ps -p $pid -o comm= 2>/dev/null)"
        done

        if [ -t 0 ]; then
            read -p "是否要停止这些进程? (y/n): " -n 1 -r
            echo
            if [[ $REPLY =~ ^[Yy]$ ]]; then
                log_info "正在停止已运行的 $APP_DISPLAY_NAME 进程..."
                echo "$APP_PIDS" | xargs kill -TERM 2>/dev/null
                sleep 2

                REMAINING_PIDS=$(pgrep -f "$APP_NAME" 2>/dev/null | grep -v pgrep 2>/dev/null)
                if [ ! -z "$REMAINING_PIDS" ]; then
                    log_warn "部分进程仍在运行，强制停止..."
                    echo "$REMAINING_PIDS" | xargs kill -9 2>/dev/null
                fi
                log_info "已停止所有 $APP_DISPLAY_NAME 进程"
            else
                log_warn "继续启动可能会与现有进程冲突"
            fi
        else
            log_warn "非交互式环境，跳过交互式确认；即将释放相关端口"
        fi
    fi

    # dev 模式需独占 Vite(18720) 与嵌入式 Axum 运维 API(18721) 端口，占用即释放
    kill_port "$VITE_PORT"
    kill_port "$AXUM_PORT"
}

# ---- 启动开发模式 ----
start_dev() {
    local FORCE_CLEAN=false
    if [ "$1" = "--clean" ]; then
        FORCE_CLEAN=true
    fi

    log_step "启动 $APP_DISPLAY_NAME 开发模式"

    check_env
    create_directories
    install_deps

    # 清理开发缓存以确保使用最新图标/配置
    log_info "清理开发缓存..."
    rm -rf "$SCRIPT_DIR/src-tauri/target/debug/bundle" 2>/dev/null

    if [ "$FORCE_CLEAN" = true ]; then
        log_info "强制清理 Rust 编译缓存..."
        cd "$SCRIPT_DIR/src-tauri"
        cargo clean >> "$DEV_LOG_FILE" 2>&1
        cd "$SCRIPT_DIR"
        log_info "Rust 编译缓存已清理"
    fi

    log_info "应用将在 Tauri 窗口中打开"
    log_info "前端开发服务器: http://localhost:$VITE_PORT"
    log_info "运维 API（Axum）: http://127.0.0.1:$AXUM_PORT"
    log_info "日志文件: $LOG_FILE（Rust/Tauri 与 Vite 输出合并）"
    log_info "按 Ctrl+C 停止应用"
    echo ""

    cd "$SCRIPT_DIR"

    export RUST_LOG=info
    export TAURI_DEV_HOST=localhost

    # 启动前再保险一次：确保端口空闲（install_deps 等步骤可能耗时，端口可能被其它进程抢占）
    kill_port "$VITE_PORT"
    kill_port "$AXUM_PORT"

    log_info "启动 Tauri 开发模式（Vite 由 beforeDevCommand 托管）..."
    # Tauri 通过 beforeDevCommand="pnpm dev" 拉起 Vite(18720)，并启动绑定 18721 的 Axum 运维服务
    pnpm tauri dev 2>> "$TAURI_LOG_FILE" &
    TAURI_BG=$!
    log_debug "Tauri 后台进程 PID: $TAURI_BG"

    # 等待 Vite(18720) 真正就绪，避免“白屏却不报错”
    local ready=false
    if command -v curl &> /dev/null; then
        for i in $(seq 1 60); do
            if curl -s -o /dev/null "http://localhost:$VITE_PORT/" 2>/dev/null; then
                ready=true
                break
            fi
            # Tauri 进程已退出 → 启动失败
            if ! kill -0 "$TAURI_BG" 2>/dev/null; then
                log_error "Tauri 开发进程异常退出，Rust/Tauri 日志如下："
                tail -40 "$TAURI_LOG_FILE"
                exit 1
            fi
            sleep 1
        done
    else
        # 无 curl 时仅等待 Tauri 进程存活
        log_warn "未检测到 curl，跳过端口就绪检测，直接等待 Tauri 进程..."
        wait "$TAURI_BG"
        return $?
    fi

    if [ "$ready" != "true" ]; then
        log_error "Vite 在 60 秒内未能就绪（端口 $VITE_PORT 无响应），Rust/Tauri 日志尾部："
        tail -40 "$TAURI_LOG_FILE"
        kill "$TAURI_BG" 2>/dev/null
        exit 1
    fi
    log_info "✅ 前端已就绪（http://localhost:$VITE_PORT），Tauri 窗口即将打开"

    # 以前台方式等待 Tauri 进程（Ctrl+C 由 trap 捕获并清理子进程）
    wait "$TAURI_BG"
    local EXIT_CODE=$?

    if [ $EXIT_CODE -ne 0 ]; then
        log_error "Tauri 开发模式异常退出（退出码: $EXIT_CODE）"
        log_error "请查看 $LOG_FILE（Rust/Tauri 与 Vite 输出合并）获取详细信息"
        log_error "---- Tauri 日志尾部 ----"
        tail -30 "$TAURI_LOG_FILE"
    fi
}

# ---- 运行测试 ----
run_tests() {
    log_step "运行 $APP_DISPLAY_NAME Rust 单元测试"

    check_env
    create_directories

    cd "$SCRIPT_DIR"

    if [ "$1" = "--all" ]; then
        log_info "运行全量测试（含集成）..."
        cargo test --manifest-path src-tauri/Cargo.toml 2> >(tee -a "$DEV_LOG_FILE" >&2)
    else
        log_info "运行库单元测试（--lib，快速）..."
        cargo test --manifest-path src-tauri/Cargo.toml --lib 2> >(tee -a "$DEV_LOG_FILE" >&2)
    fi

    if [ $? -ne 0 ]; then
        log_error "测试失败，查看 $DEV_LOG_FILE"
        exit 1
    fi
    log_info "测试全部通过"
}

# ---- 构建应用 ----
build_app() {
    log_step "构建 $APP_DISPLAY_NAME"

    check_env
    create_directories
    install_deps

    log_info "正在构建 $APP_DISPLAY_NAME（Windows 安装包：msi / nsis）..."

    cd "$SCRIPT_DIR"

    pnpm tauri build 2>> "$TAURI_LOG_FILE"

    if [ $? -ne 0 ]; then
        log_error "构建失败，查看 $TAURI_LOG_FILE"
        exit 1
    fi

    log_info "构建成功！"
    log_info "构建产物位于: $SCRIPT_DIR/src-tauri/target/release/bundle/"
    log_info "注意：在 macOS/Linux 上构建 Windows 包需 windows-msvc 目标与 rc.exe（资源编译器），"
    log_info "      通常交由 .github/workflows/windows-build.yml 在 windows-latest 上完成。"
}

# ---- 清理日志 ----
clean_logs() {
    log_info "清理日志文件..."
    rm -f "$LOG_DIR"/*.log
    log_info "日志已清理"
}

# ---- 查看日志 ----
view_logs() {
    if [ -f "$LOG_FILE" ]; then
        log_info "最近 50 行应用日志:"
        tail -50 "$LOG_FILE"
    else
        log_warn "日志文件不存在: $LOG_FILE"
    fi
}

# ---- 帮助信息 ----
show_help() {
    echo ""
    echo -e "${GREEN}$APP_DISPLAY_NAME 开发/构建脚本${NC}"
    echo ""
    echo "用法: $0 [命令] [选项]"
    echo ""
    echo "命令:"
    echo "  dev                启动开发模式（默认，Tauri + Vite，内嵌 Axum 运维 API）"
    echo "  dev --clean       强制清理 Rust 编译缓存后启动开发模式"
    echo "  test              运行 Rust 单元测试（--lib，快速）"
    echo "  test --all        运行全量测试（含集成）"
    echo "  build             构建生产版本（Windows msi / nsis 安装包）"
    echo "  clean             清理日志文件"
    echo "  logs              查看最近的应用日志"
    echo "  help, -h, --help  显示此帮助信息"
    echo ""
    echo "端口说明:"
    echo "  前端开发: http://localhost:$VITE_PORT（Vite，strictPort）"
    echo "  运维 API: http://127.0.0.1:$AXUM_PORT（内嵌 Axum，仅本机）"
    echo ""
    echo "日志文件:"
    echo "  $LOG_FILE"
    echo ""
    echo "示例:"
    echo "  $0              # 启动开发模式"
    echo "  $0 dev          # 启动开发模式"
    echo "  $0 test         # 跑单元测试"
    echo "  $0 build        # 构建生产版本"
    echo "  $0 logs         # 查看日志"
    echo "  $0 clean        # 清理日志"
    echo ""
}

# ---- 主函数 ----
main() {
    # 确保所有子命令的日志写入都能落到 logs/ 目录（不依赖 create_directories 是否已被调用）
    mkdir -p "$LOG_DIR"

    case "${1:-dev}" in
        dev)
            log_info "$APP_DISPLAY_NAME 开发模式启动"
            echo "========================================"
            check_and_stop_processes
            start_dev "$2"
            ;;
        test)
            log_info "$APP_DISPLAY_NAME 运行测试"
            echo "========================================"
            run_tests "$2"
            ;;
        build)
            log_info "$APP_DISPLAY_NAME 构建"
            echo "========================================"
            build_app
            ;;
        clean)
            clean_logs
            ;;
        logs)
            view_logs
            ;;
        help|-h|--help)
            show_help
            ;;
        *)
            log_error "未知命令: $1"
            echo "使用 '$0 help' 查看帮助信息"
            exit 1
            ;;
    esac
}

main "$@"
