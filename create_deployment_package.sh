#!/bin/bash

# Create deployment package for Raspberry Pi 5
# This script creates a tarball with source code and build instructions
# for compilation directly on the Pi

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_NAME="drm-vc4-grabber-source"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
PACKAGE_DIR="${PACKAGE_NAME}_${TIMESTAMP}"
TARBALL="${PACKAGE_DIR}.tar.gz"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() {
    echo -e "[$(date '+%H:%M:%S')] $1"
}

log_success() {
    echo -e "[$(date '+%H:%M:%S')] ${GREEN}SUCCESS: $1${NC}"
}

log_info() {
    echo -e "[$(date '+%H:%M:%S')] ${BLUE}INFO: $1${NC}"
}

log_warning() {
    echo -e "[$(date '+%H:%M:%S')] ${YELLOW}WARNING: $1${NC}"
}

create_deployment_package() {
    log_info "Creating deployment package for Raspberry Pi 5..."
    
    # Create temporary directory
    mkdir -p "$PACKAGE_DIR"
    
    # Copy source files
    log_info "Copying source files..."
    cp -r src/ "$PACKAGE_DIR/"
    cp Cargo.toml "$PACKAGE_DIR/"
    cp Cargo.lock "$PACKAGE_DIR/"
    cp *.md "$PACKAGE_DIR/"
    cp *.sh "$PACKAGE_DIR/"
    
    # Copy systemd service file
    if [ -d "systemd" ]; then
        cp -r systemd/ "$PACKAGE_DIR/"
    fi
    
    # Create Pi-specific build script
    cat > "$PACKAGE_DIR/build_on_pi.sh" << 'EOF'
#!/bin/bash

# Build script for Raspberry Pi 5
# Run this script on the Pi to build the grabber

set -e

log() {
    echo "[$(date '+%H:%M:%S')] $1"
}

log_error() {
    echo "[$(date '+%H:%M:%S')] ERROR: $1" >&2
}

log_success() {
    echo "[$(date '+%H:%M:%S')] SUCCESS: $1"
}

log_info() {
    echo "[$(date '+%H:%M:%S')] INFO: $1"
}

check_prerequisites() {
    log_info "Checking prerequisites on Raspberry Pi..."
    
    # Check if we're on ARM64
    if [ "$(uname -m)" != "aarch64" ]; then
        log_error "This script should be run on ARM64 (aarch64) architecture"
        log_info "Current architecture: $(uname -m)"
        exit 1
    fi
    
    # Check if Rust is installed
    if ! command -v rustc &> /dev/null; then
        log_error "Rust is not installed. Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source ~/.cargo/env
        
        if ! command -v rustc &> /dev/null; then
            log_error "Failed to install Rust. Please install manually."
            exit 1
        fi
    fi
    
    log_success "Prerequisites check passed"
    log_info "Rust version: $(rustc --version)"
    log_info "Cargo version: $(cargo --version)"
}

build_project() {
    log_info "Building DRM VC4 Grabber on Raspberry Pi..."
    
    # Clean any previous builds
    if [ -d "target" ]; then
        log_info "Cleaning previous build..."
        cargo clean
    fi
    
    # Build in release mode
    log_info "Building in release mode..."
    if cargo build --release; then
        log_success "Build completed successfully"
    else
        log_error "Build failed"
        exit 1
    fi
    
    # Show build info
    if [ -f "target/release/drm-vc4-grabber" ]; then
        log_success "Binary created: target/release/drm-vc4-grabber"
        log_info "Binary size: $(du -h target/release/drm-vc4-grabber | cut -f1)"
        
        # Make executable
        chmod +x target/release/drm-vc4-grabber
        chmod +x diagnostic_test.sh
        
        echo ""
        echo "=========================================="
        echo "BUILD COMPLETED SUCCESSFULLY"
        echo "=========================================="
        echo "Binary location: target/release/drm-vc4-grabber"
        echo ""
        echo "Next steps:"
        echo "1. Test the grabber:"
        echo "   sudo ./target/release/drm-vc4-grabber --help"
        echo ""
        echo "2. Run diagnostic test:"
        echo "   sudo ./diagnostic_test.sh"
        echo ""
        echo "3. Run with diagnostic logging:"
        echo "   sudo ./target/release/drm-vc4-grabber --diagnostic --verbose"
        echo ""
        echo "4. Install systemd service (optional):"
        echo "   sudo cp systemd/drm-capture.service /etc/systemd/system/"
        echo "   sudo systemctl enable drm-capture.service"
        echo "=========================================="
    else
        log_error "Binary not found after build"
        exit 1
    fi
}

main() {
    log_info "Starting build process on Raspberry Pi 5"
    check_prerequisites
    build_project
    log_success "Build process completed"
}

main "$@"
EOF
    
    chmod +x "$PACKAGE_DIR/build_on_pi.sh"
    
    # Create deployment instructions
    cat > "$PACKAGE_DIR/DEPLOYMENT_INSTRUCTIONS.md" << 'EOF'
# Deployment Instructions for Raspberry Pi 5

## Prerequisites

1. **Raspberry Pi 5** with a 64-bit OS (Raspberry Pi OS 64-bit, Ubuntu, etc.)
2. **Root access** (sudo privileges)
3. **Internet connection** (for Rust installation if needed)

## Deployment Steps

### 1. Transfer Files to Pi

Copy this entire directory to your Raspberry Pi 5:

```bash
# From your local machine:
scp -r drm-vc4-grabber-source_* pi@your-pi-ip:~/

# Or use rsync:
rsync -av drm-vc4-grabber-source_* pi@your-pi-ip:~/
```

### 2. Build on Pi

SSH into your Pi and build the project:

```bash
ssh pi@your-pi-ip
cd drm-vc4-grabber-source_*
./build_on_pi.sh
```

The build script will:
- Check if Rust is installed (install if needed)
- Build the project in release mode
- Set up executable permissions
- Provide next steps

### 3. Test Installation

After building, test the installation:

```bash
# Test basic functionality
sudo ./target/release/drm-vc4-grabber --help

# Run diagnostic test (recommended)
sudo ./diagnostic_test.sh --duration 300

# Run with diagnostic logging
sudo ./target/release/drm-vc4-grabber --diagnostic --verbose
```

### 4. Install as Service (Optional)

To run automatically at boot:

```bash
sudo cp systemd/drm-capture.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable drm-capture.service
sudo systemctl start drm-capture.service
```

## Troubleshooting

### Build Issues

If the build fails:

1. **Update Rust**: `rustup update`
2. **Install dependencies**: `sudo apt update && sudo apt install build-essential`
3. **Check disk space**: `df -h`

### Runtime Issues

If the grabber doesn't work:

1. **Check permissions**: Must run as root (`sudo`)
2. **Check DRM device**: `ls -la /dev/dri/`
3. **Check Kodi status**: `systemctl status kodi`
4. **Run diagnostic test**: `sudo ./diagnostic_test.sh`

### Getting Help

1. Check the diagnostic logs in `diagnostic_logs/`
2. Run with `--verbose` flag for detailed output
3. Check system logs: `dmesg | grep drm`

## Architecture Notes

This version includes:
- **Comprehensive diagnostic system** for stability analysis
- **File-based logging** instead of syslog
- **System monitoring** for Kodi interaction analysis
- **Resource tracking** to detect leaks
- **Automated testing** framework

The diagnostic system is specifically designed to help identify and resolve the Pi 5 stability issues when running alongside Kodi.
EOF
    
    # Create the tarball
    log_info "Creating tarball..."
    tar -czf "$TARBALL" "$PACKAGE_DIR"
    
    # Clean up temporary directory
    rm -rf "$PACKAGE_DIR"
    
    log_success "Deployment package created: $TARBALL"
    
    # Show package info
    echo ""
    echo "=========================================="
    echo "DEPLOYMENT PACKAGE READY"
    echo "=========================================="
    echo "Package: $TARBALL"
    echo "Size: $(du -h "$TARBALL" | cut -f1)"
    echo ""
    echo "To deploy to Raspberry Pi 5:"
    echo "1. Copy to Pi: scp $TARBALL pi@your-pi-ip:~/"
    echo "2. Extract: tar -xzf $TARBALL"
    echo "3. Build: cd ${PACKAGE_DIR%_*}_* && ./build_on_pi.sh"
    echo "4. Test: sudo ./diagnostic_test.sh"
    echo "=========================================="
}

main() {
    log_info "Creating deployment package for Raspberry Pi 5"
    create_deployment_package
}

main "$@"