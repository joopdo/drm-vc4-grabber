#!/bin/bash

# DRM VC4 Grabber Build Script
# Automates the build process for Raspberry Pi 5

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_ARCH="aarch64-unknown-linux-gnu"
BUILD_TYPE="release"
CROSS_COMPILE=false

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

check_rust_installation() {
    log_info "Checking Rust installation..."
    
    if ! command -v rustc &> /dev/null; then
        log_error "Rust is not installed. Please install Rust first:"
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi
    
    if ! command -v cargo &> /dev/null; then
        log_error "Cargo is not installed. Please install Rust toolchain."
        exit 1
    fi
    
    log_success "Rust installation found"
    log_info "Rust version: $(rustc --version)"
    log_info "Cargo version: $(cargo --version)"
}

setup_cross_compilation() {
    if [ "$CROSS_COMPILE" = true ]; then
        log_info "Setting up cross-compilation for $TARGET_ARCH..."
        
        # Install target
        log_info "Installing target: $TARGET_ARCH"
        rustup target add "$TARGET_ARCH"
        
        # Check for cross-compiler
        local linker="/usr/bin/aarch64-linux-gnu-gcc"
        if [ ! -f "$linker" ]; then
            log_warning "Cross-compiler not found at $linker"
            log_info "Cross-compilation setup failed. This is common on some systems."
            log_info ""
            log_info "Alternative options:"
            log_info "1. Use deployment package: ./create_deployment_package.sh"
            log_info "2. Install cross-compiler: sudo apt install gcc-aarch64-linux-gnu"
            log_info "3. Build natively on the Raspberry Pi"
            log_info ""
            log_info "Creating deployment package instead..."
            
            # Automatically create deployment package
            if [ -f "./create_deployment_package.sh" ]; then
                ./create_deployment_package.sh
                log_success "Deployment package created. Copy it to your Pi and build there."
                exit 0
            else
                log_error "Deployment package script not found"
                exit 1
            fi
        fi
        
        # Set linker environment variable
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$linker"
        log_success "Cross-compilation setup complete"
    else
        log_info "Building natively (no cross-compilation)"
    fi
}

build_project() {
    log_info "Building project..."
    
    cd "$SCRIPT_DIR"
    
    # Clean previous builds if requested
    if [ "$CLEAN_BUILD" = true ]; then
        log_info "Cleaning previous build..."
        cargo clean
    fi
    
    # Build command
    local build_cmd="cargo build"
    
    if [ "$BUILD_TYPE" = "release" ]; then
        build_cmd="$build_cmd --release"
    fi
    
    if [ "$CROSS_COMPILE" = true ]; then
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
    if [ "$CROSS_COMPILE" = true ]; then
        binary_path="target/$TARGET_ARCH/$BUILD_TYPE/drm-vc4-grabber"
    else
        binary_path="target/$BUILD_TYPE/drm-vc4-grabber"
    fi
    
    if [ -f "$binary_path" ]; then
        log_success "Binary created: $binary_path"
        log_info "Binary size: $(du -h "$binary_path" | cut -f1)"
        
        if command -v file &> /dev/null; then
            log_info "Binary info: $(file "$binary_path")"
        fi
        
        echo ""
        echo "To run the grabber:"
        echo "  sudo ./$binary_path --help"
        echo ""
        echo "To run diagnostic test:"
        echo "  sudo ./diagnostic_test.sh"
        echo ""
        echo "To run with diagnostic logging:"
        echo "  sudo ./$binary_path --diagnostic --verbose"
        
    else
        log_error "Binary not found at expected location: $binary_path"
        exit 1
    fi
}

run_tests() {
    if [ "$RUN_TESTS" = true ]; then
        log_info "Running tests..."
        
        if cargo test; then
            log_success "All tests passed"
        else
            log_warning "Some tests failed"
        fi
    fi
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --cross)
            CROSS_COMPILE=true
            shift
            ;;
        --debug)
            BUILD_TYPE="debug"
            shift
            ;;
        --clean)
            CLEAN_BUILD=true
            shift
            ;;
        --test)
            RUN_TESTS=true
            shift
            ;;
        --target)
            TARGET_ARCH="$2"
            CROSS_COMPILE=true
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo "Options:"
            echo "  --cross              Enable cross-compilation for ARM64"
            echo "  --debug              Build in debug mode (default: release)"
            echo "  --clean              Clean before building"
            echo "  --test               Run tests after building"
            echo "  --target ARCH        Specify target architecture (implies --cross)"
            echo "  -h, --help          Show this help"
            echo ""
            echo "Examples:"
            echo "  $0                   # Native release build"
            echo "  $0 --cross           # Cross-compile for ARM64"
            echo "  $0 --debug --test    # Debug build with tests"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Main execution
main() {
    log_info "Starting DRM VC4 Grabber build process"
    
    check_rust_installation
    setup_cross_compilation
    run_tests
    build_project
    show_build_info
    
    log_success "Build process completed"
}

main "$@"