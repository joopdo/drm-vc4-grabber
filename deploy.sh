#!/bin/bash

# Quick deployment script for DRM VC4 Grabber
# Deploys pre-built binary to Raspberry Pi

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_ARCH="aarch64-unknown-linux-musl"
BUILD_TYPE="release"
PI_HOST=""
PI_USER="pi"
PI_BINARY_PATH="/storage/drm-vc4-grabber"

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

deploy_to_pi() {
    if [ -z "$PI_HOST" ]; then
        log_error "PI_HOST not specified. Use: $0 pi@hostname"
        exit 1
    fi
    
    local binary_path="target/$TARGET_ARCH/$BUILD_TYPE/drm-vc4-grabber"
    
    if [ ! -f "$binary_path" ]; then
        log_error "Binary not found at: $binary_path"
        log_info "Run './build.sh --cross' first to build the binary"
        exit 1
    fi
    
    log_info "Deploying to Raspberry Pi: $PI_HOST"
    log_info "Binary: $binary_path -> $PI_BINARY_PATH"
    
    # Detect if target is LibreELEC (no real sudo, runs as root)
    log_info "Detecting target system..."
    local has_sudo=true
    
    # LibreELEC has a fake sudo that always fails, so test if it actually works
    # The fake sudo script always exits with code 1, so we need to check the exit code
    if ssh "$PI_HOST" "sudo echo 'test' >/dev/null 2>&1"; then
        log_info "Target system has working sudo"
    else
        # Check if we're already running as root (LibreELEC default)
        if ssh "$PI_HOST" "id -u" | grep -q "^0$"; then
            log_info "Target system is running as root (LibreELEC detected)"
            has_sudo=false
        else
            log_warning "Target system sudo test failed, but not running as root"
        fi
    fi
    
    # Stop existing process
    log_info "Stopping existing grabber process on Pi..."
    ssh "$PI_HOST" "pkill -f drm-vc4-grabber || true" || log_warning "Could not stop existing process (may not be running)"
    
    # Create target directory if it doesn't exist
    local target_dir=$(dirname "$PI_BINARY_PATH")
    log_info "Ensuring target directory exists: $target_dir"
    ssh "$PI_HOST" "mkdir -p $target_dir"
    
    # Copy binary to Pi
    log_info "Copying binary to Pi..."
    scp "$binary_path" "$PI_HOST:$PI_BINARY_PATH"
    
    log_info "Monitoring and diagnostic functionality is built into the main binary"
    
    # Set permissions
    log_info "Setting executable permissions..."
    ssh "$PI_HOST" "chmod +x $PI_BINARY_PATH"
    
    log_success "Deployment completed successfully"
}

run_connection_tests() {
    log_info "Running connection reliability tests on Pi..."
    
    # Test 1: Verify new options are available
    if ssh "$PI_HOST" "$PI_BINARY_PATH --help" | grep -q "max-retries"; then
        log_success "Connection management options available"
    else
        log_error "Connection management options not found"
        return 1
    fi
    
    # Test 2: Quick connection test with invalid address (fallback mode)
    log_info "Testing fallback mode with invalid Hyperion address..."
    
    # Start Kodi monitoring in background
    start_kodi_monitoring
    
    ssh "$PI_HOST" "timeout 10s $PI_BINARY_PATH --diagnostic --address 127.0.0.1:99999 --max-retries 2 --connection-timeout 500 --fps 5 --log-file connection-test.log" || {
        log_info "Test completed (expected timeout)"
        
        # Check if fallback mode was activated
        if ssh "$PI_HOST" "grep -q 'fallback\\|Operating without Hyperion' connection-test.log 2>/dev/null"; then
            log_success "Fallback mode activated correctly"
        else
            log_warning "Fallback mode not detected in logs"
        fi
    }
    
    # Stop Kodi monitoring
    stop_kodi_monitoring
    
    log_success "Connection reliability tests completed"
}

start_kodi_monitoring() {
    log_info "Starting Kodi crash monitoring..."
    
    # Create Kodi monitoring script on Pi
    ssh "$PI_HOST" 'cat > /tmp/kodi_monitor.sh << '\''EOF'\''
#!/bin/bash
KODI_LOG="/storage/.kodi/temp/kodi.log"
DIAGNOSTIC_LOG="/tmp/kodi-diagnostic.log"

echo "=== Kodi Monitor Started $(date) ===" >> "$DIAGNOSTIC_LOG"

check_kodi_status() {
    if pgrep -f kodi > /dev/null; then
        echo "RUNNING"
    else
        echo "NOT_RUNNING"
    fi
}

LAST_STATUS=$(check_kodi_status)
echo "Initial Kodi status: $LAST_STATUS" >> "$DIAGNOSTIC_LOG"

while [ -f /tmp/kodi_monitor_active ]; do
    CURRENT_STATUS=$(check_kodi_status)
    
    if [ "$CURRENT_STATUS" != "$LAST_STATUS" ]; then
        echo "[$(date)] Kodi status: $LAST_STATUS -> $CURRENT_STATUS" >> "$DIAGNOSTIC_LOG"
        
        if [ "$CURRENT_STATUS" = "NOT_RUNNING" ]; then
            echo "[$(date)] KODI CRASH DETECTED" >> "$DIAGNOSTIC_LOG"
            if [ -f "$KODI_LOG" ]; then
                tail -n 20 "$KODI_LOG" | grep -i -E "(error|drm|crash)" >> "$DIAGNOSTIC_LOG" 2>/dev/null || true
            fi
        fi
        
        LAST_STATUS="$CURRENT_STATUS"
    fi
    
    sleep 2
done
EOF'
    
    # Start monitoring
    ssh "$PI_HOST" "touch /tmp/kodi_monitor_active && chmod +x /tmp/kodi_monitor.sh && /tmp/kodi_monitor.sh &"
}

stop_kodi_monitoring() {
    log_info "Stopping Kodi monitoring..."
    ssh "$PI_HOST" "rm -f /tmp/kodi_monitor_active; pkill -f kodi_monitor.sh 2>/dev/null || true"
}

collect_diagnostic_logs() {
    log_info "Collecting diagnostic logs from Pi..."
    
    local logs_dir="pi_logs_$(date +%Y%m%d_%H%M%S)"
    mkdir -p "$logs_dir"
    
    # Collect diagnostic logs
    ssh "$PI_HOST" "find /storage -name '*.log' -newer /tmp 2>/dev/null || ls -t /storage/*.log 2>/dev/null | head -5" | while read -r log_file; do
        if [ -n "$log_file" ]; then
            local basename=$(basename "$log_file")
            log_info "Collecting: $basename"
            scp "$PI_HOST:$log_file" "$logs_dir/" 2>/dev/null || true
        fi
    done
    
    # Collect system info
    ssh "$PI_HOST" "uname -a; free -h; ps aux | grep -E '(kodi|hyperion|drm)'" > "$logs_dir/system_info.txt" 2>/dev/null || true
    
    log_success "Logs collected in: $logs_dir/"
    
    # Quick analysis
    if [ -f "$logs_dir"/*.log ]; then
        local broken_pipes=$(grep -c "Broken pipe" "$logs_dir"/*.log 2>/dev/null || echo "0")
        local reconnections=$(grep -c "reconnect" "$logs_dir"/*.log 2>/dev/null || echo "0")
        local fallbacks=$(grep -c "fallback" "$logs_dir"/*.log 2>/dev/null || echo "0")
        
        echo ""
        log_info "Quick Analysis:"
        log_info "  Broken pipe errors: $broken_pipes"
        log_info "  Reconnection attempts: $reconnections" 
        log_info "  Fallback activations: $fallbacks"
        
        if [ "$broken_pipes" -gt 0 ] && [ "$reconnections" -gt 0 ]; then
            log_success "Connection recovery working correctly"
        elif [ "$broken_pipes" -eq 0 ]; then
            log_success "No connection issues detected"
        fi
    fi
    
    log_success "Deployment completed successfully"
    
    echo ""
    echo "=========================================="
    echo "DEPLOYMENT COMPLETED"
    echo "=========================================="
    echo "Binary deployed to: $PI_HOST:$PI_BINARY_PATH"
    echo ""
    echo "To test on Pi:"
    echo "  ssh $PI_HOST"
    if [ "$has_sudo" = true ]; then
        echo "  sudo $PI_BINARY_PATH --help"
        echo "  sudo $PI_BINARY_PATH --diagnostic --verbose"
    else
        echo "  $PI_BINARY_PATH --help"
        echo "  $PI_BINARY_PATH --diagnostic --verbose"
    fi
    echo "=========================================="
}

# Parse command line arguments
if [ $# -eq 0 ]; then
    echo "Usage: $0 [USER@]HOST [OPTIONS]"
    echo ""
    echo "Arguments:"
    echo "  HOST                 Raspberry Pi hostname or IP"
    echo "  USER@HOST            Username and hostname (default user: pi)"
    echo ""
    echo "Options:"
    echo "  --pi-path PATH       Path on Pi where binary should be deployed"
    echo "                       (default: /home/pi/drm-vc4-grabber)"
    echo "  --debug              Deploy debug binary instead of release"
    echo "  --test               Run connection reliability tests after deployment"
    echo "  --collect-logs       Collect diagnostic logs from Pi after testing"
    echo ""
    echo "Examples:"
    echo "  $0 192.168.1.100                    # Deploy to pi@192.168.1.100"
    echo "  $0 pi@raspberrypi.local             # Deploy to pi@raspberrypi.local"
    echo "  $0 mypi --pi-path /usr/local/bin/grabber  # Custom path"
    exit 1
fi

while [[ $# -gt 0 ]]; do
    case $1 in
        --pi-path)
            PI_BINARY_PATH="$2"
            shift 2
            ;;
        --debug)
            BUILD_TYPE="debug"
            shift
            ;;
        --test)
            RUN_TESTS="true"
            shift
            ;;
        --collect-logs)
            COLLECT_LOGS="true"
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [USER@]HOST [OPTIONS]"
            echo "Quick deployment script for pre-built binaries"
            exit 0
            ;;
        *)
            if [ -z "$PI_HOST" ]; then
                PI_HOST="$1"
                # Add default user if not specified
                if [[ "$PI_HOST" != *"@"* ]]; then
                    PI_HOST="$PI_USER@$PI_HOST"
                fi
            else
                log_error "Unknown option: $1"
                exit 1
            fi
            shift
            ;;
    esac
done

# Main execution
main() {
    log_info "Starting deployment to Raspberry Pi"
    deploy_to_pi
    
    # Run tests if requested
    if [ "$RUN_TESTS" = "true" ]; then
        run_connection_tests
    fi
    
    # Collect logs if requested
    if [ "$COLLECT_LOGS" = "true" ]; then
        collect_diagnostic_logs
    fi
}

main "$@"
