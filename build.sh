#!/bin/bash

# DRM VC4 Grabber Build Script
# Supports local builds, cross-compilation, and direct deployment

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_ARCH="aarch64-unknown-linux-musl"
BUILD_TYPE="release"
DEPLOY_HOST=""
DEPLOY_PATH="/storage/drm-vc4-grabber"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() {
    echo -e "[$(date '+%H:%M:%S')] $1"
}

log_error() {
    echo -e "[$(date '+%H:%M:%S')] ${RED}ERROR: $1${NC}"
}

log_warning() {
    echo -e "[$(date '+%H:%M:%S')] ${YELLOW}WARNING: $1${NC}"
}

log_success() {
    echo -e "[$(date '+%H:%M:%S')] ${GREEN}SUCCESS: $1${NC}"
}

log_info() {
    echo -e "[$(date '+%H:%M:%S')] ${BLUE}INFO: $1${NC}"
}

show_help() {
    echo "DRM VC4 Grabber Build Script"
    echo ""
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Build Options:"
    echo "  --local              Build for local architecture (default)"
    echo "  --cross              Cross-compile for Raspberry Pi (aarch64-musl)"
    echo "  --debug              Build debug version instead of release"
    echo "  --musl               Use musl target (static linking)"
    echo ""
    echo "Deployment Options:"
    echo "  --deploy HOST        Deploy to Raspberry Pi after building"
    echo "  --pi-path PATH       Custom path on Pi (default: /storage/drm-vc4-grabber)"
    echo ""
    echo "Testing Options:"
    echo "  --diagnostic-test    Run diagnostic tests after deployment"
    echo "  --stability-test     Run stability tests after deployment"
    echo "  --collect-logs       Collect diagnostic logs from Pi"
    echo ""
    echo "Examples:"
    echo "  $0                                    # Local build"
    echo "  $0 --cross                           # Cross-compile for Pi"
    echo "  $0 --cross --deploy pi@192.168.1.100 # Build and deploy"
    echo "  $0 --debug --local                   # Local debug build"
    exit 0
}

check_rust() {
    log_info "Checking Rust installation..."
    
    if ! command -v rustc &> /dev/null; then
        log_error "Rust not found. Please install Rust: https://rustup.rs/"
        exit 1
    fi
    
    if command -v rustup &> /dev/null; then
        log_success "Rust installation found (rustup managed)"
    else
        log_success "Rust installation found (system managed)"
    fi
    
    log_info "Rust version: $(rustc --version)"
    log_info "Cargo version: $(cargo --version)"
    log_info "Rust location: $(which rustc)"
}

setup_cross_compilation() {
    log_info "Setting up cross-compilation for $TARGET_ARCH..."
    
    # Check if cross-compilation toolchain is available
    if [ "$TARGET_ARCH" = "aarch64-unknown-linux-musl" ]; then
        if ! command -v aarch64-linux-gnu-gcc &> /dev/null; then
            log_error "Cross-compilation toolchain not found!"
            log_error "Please install the aarch64 cross-compilation toolchain:"
            log_error "  Ubuntu/Debian: sudo apt install gcc-aarch64-linux-gnu"
            log_error "  Or use cross tool: cargo install cross"
            exit 1
        fi
        
        # Create .cargo/config.toml if it doesn't exist
        mkdir -p .cargo
        if [ ! -f .cargo/config.toml ]; then
            log_info "Creating Cargo cross-compilation config..."
            cat > .cargo/config.toml << 'EOF'
[target.aarch64-unknown-linux-musl]
linker = "aarch64-linux-gnu-gcc"

[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
EOF
        fi
    fi
    
    if command -v rustup &> /dev/null; then
        log_info "Using rustup - installing target: $TARGET_ARCH"
        rustup target add $TARGET_ARCH
    else
        log_warning "rustup not found - assuming target is already available"
    fi
    
    log_success "Cross-compilation setup complete"
}

build_project() {
    log_info "Building project..."
    
    local build_cmd
    local use_cross=false
    
    # Check if we should use cross tool for cross-compilation
    if [ "$CROSS_COMPILE" = "true" ]; then
        if command -v cross &> /dev/null; then
            log_info "Using 'cross' tool for cross-compilation"
            build_cmd="cross build"
            use_cross=true
        elif command -v aarch64-linux-gnu-gcc &> /dev/null; then
            log_info "Using native cross-compilation toolchain"
            build_cmd="cargo build"
        else
            log_error "No cross-compilation method available!"
            log_error "Install either:"
            log_error "  1. cross tool: cargo install cross"
            log_error "  2. gcc toolchain: sudo apt install gcc-aarch64-linux-gnu"
            exit 1
        fi
    else
        build_cmd="cargo build"
    fi
    
    if [ "$BUILD_TYPE" = "release" ]; then
        build_cmd="$build_cmd --release"
    fi
    
    if [ "$CROSS_COMPILE" = "true" ] && [ "$use_cross" = "false" ]; then
        build_cmd="$build_cmd --target $TARGET_ARCH"
    elif [ "$CROSS_COMPILE" = "true" ] && [ "$use_cross" = "true" ]; then
        build_cmd="$build_cmd --target $TARGET_ARCH"
    fi
    
    log_info "Running: $build_cmd"
    
    if $build_cmd; then
        log_success "Build completed successfully"
    else
        log_error "Build failed"
        exit 1
    fi
}

show_build_info() {
    log_info "Build information:"
    
    local binary_path
    if [ "$CROSS_COMPILE" = "true" ]; then
        binary_path="target/$TARGET_ARCH/$BUILD_TYPE/drm-vc4-grabber"
    else
        binary_path="target/$BUILD_TYPE/drm-vc4-grabber"
    fi
    
    if [ -f "$binary_path" ]; then
        log_success "Binary created: $binary_path"
        
        local size=$(du -h "$binary_path" | cut -f1)
        log_info "Binary size: $size"
        
        if command -v file &> /dev/null; then
            local file_info=$(file "$binary_path")
            log_info "Binary info: $file_info"
        fi
    else
        log_error "Binary not found at expected path: $binary_path"
        exit 1
    fi
}

deploy_to_pi() {
    if [ -z "$DEPLOY_HOST" ]; then
        log_error "No deployment host specified"
        exit 1
    fi
    
    log_info "Deploying to Raspberry Pi..."
    log_info "Running deployment with: ./deploy.sh $DEPLOY_HOST --pi-path $DEPLOY_PATH"
    
    local deploy_args="$DEPLOY_HOST --pi-path $DEPLOY_PATH"
    
    if [ "$BUILD_TYPE" = "debug" ]; then
        deploy_args="$deploy_args --debug"
    fi
    
    if [ "$RUN_DIAGNOSTIC_TEST" = "true" ]; then
        deploy_args="$deploy_args --test"
    fi
    
    if [ "$COLLECT_LOGS" = "true" ]; then
        deploy_args="$deploy_args --collect-logs"
    fi
    
    if [ -f "./deploy.sh" ]; then
        ./deploy.sh $deploy_args
    else
        log_error "deploy.sh not found"
        exit 1
    fi
}

# Parse command line arguments
CROSS_COMPILE=false
RUN_DIAGNOSTIC_TEST=false
RUN_STABILITY_TEST=false
COLLECT_LOGS=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --local)
            CROSS_COMPILE=false
            shift
            ;;
        --cross)
            CROSS_COMPILE=true
            shift
            ;;
        --debug)
            BUILD_TYPE="debug"
            shift
            ;;
        --musl)
            TARGET_ARCH="aarch64-unknown-linux-musl"
            shift
            ;;
        --deploy)
            DEPLOY_HOST="$2"
            shift 2
            ;;
        --pi-path)
            DEPLOY_PATH="$2"
            shift 2
            ;;
        --diagnostic-test)
            RUN_DIAGNOSTIC_TEST=true
            shift
            ;;
        --stability-test)
            RUN_STABILITY_TEST=true
            shift
            ;;
        --collect-logs)
            COLLECT_LOGS=true
            shift
            ;;
        -h|--help)
            show_help
            ;;
        *)
            log_error "Unknown option: $1"
            show_help
            ;;
    esac
done

# Auto-enable cross-compilation if deployment is requested
if [ -n "$DEPLOY_HOST" ] && [ "$CROSS_COMPILE" = "false" ]; then
    log_info "Deployment requested - enabling cross-compilation"
    CROSS_COMPILE=true
fi

# Main execution
main() {
    log_info "Starting DRM VC4 Grabber build process"
    
    check_rust
    
    if [ "$CROSS_COMPILE" = "true" ]; then
        setup_cross_compilation
    fi
    
    build_project
    show_build_info
    
    if [ -n "$DEPLOY_HOST" ]; then
        deploy_to_pi
    fi
    
    log_success "Build process completed successfully"
}

main "$@"