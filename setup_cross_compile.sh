#!/bin/bash

# Setup script for cross-compilation on build machine
# Run this once to install the necessary tools

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log() {
    echo -e "[$(date '+%H:%M:%S')] $1"
}

log_error() {
    echo -e "[$(date '+%H:%M:%S')] ${RED}ERROR: $1${NC}"
}

log_success() {
    echo -e "[$(date '+%H:%M:%S')] ${GREEN}SUCCESS: $1${NC}"
}

log_info() {
    echo -e "[$(date '+%H:%M:%S')] ${BLUE}INFO: $1${NC}"
}

log_info "Setting up cross-compilation environment..."

# Check if we're on a supported system
if command -v apt &> /dev/null; then
    log_info "Detected Debian/Ubuntu system"
    
    log_info "Installing aarch64 cross-compilation toolchain..."
    sudo apt update
    sudo apt install -y gcc-aarch64-linux-gnu
    
    log_success "Cross-compilation toolchain installed"
    
elif command -v yum &> /dev/null || command -v dnf &> /dev/null; then
    log_info "Detected Red Hat/Fedora system"
    
    if command -v dnf &> /dev/null; then
        sudo dnf install -y gcc-aarch64-linux-gnu
    else
        sudo yum install -y gcc-aarch64-linux-gnu
    fi
    
    log_success "Cross-compilation toolchain installed"
    
else
    log_info "Unknown package manager, trying alternative approach..."
    
    # Install cross tool as fallback
    if command -v cargo &> /dev/null; then
        log_info "Installing 'cross' tool as alternative..."
        cargo install cross
        log_success "'cross' tool installed"
    else
        log_error "Cannot install cross-compilation tools automatically"
        log_error "Please install manually:"
        log_error "  Option 1: Install gcc-aarch64-linux-gnu package"
        log_error "  Option 2: Install cross tool: cargo install cross"
        exit 1
    fi
fi

# Verify installation
log_info "Verifying installation..."

if command -v aarch64-linux-gnu-gcc &> /dev/null; then
    log_success "aarch64-linux-gnu-gcc found: $(which aarch64-linux-gnu-gcc)"
elif command -v cross &> /dev/null; then
    log_success "cross tool found: $(which cross)"
else
    log_error "No cross-compilation tools found after installation"
    exit 1
fi

# Create Cargo config
log_info "Creating Cargo cross-compilation config..."
mkdir -p .cargo

cat > .cargo/config.toml << 'EOF'
[target.aarch64-unknown-linux-musl]
linker = "aarch64-linux-gnu-gcc"

[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
EOF

log_success "Cargo config created"

# Add Rust target
if command -v rustup &> /dev/null; then
    log_info "Adding aarch64-unknown-linux-musl target..."
    rustup target add aarch64-unknown-linux-musl
    log_success "Rust target added"
fi

log_success "Cross-compilation setup complete!"
log_info "You can now run: ./build.sh --cross --deploy root@192.168.2.2"