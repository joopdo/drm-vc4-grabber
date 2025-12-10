#!/bin/bash

# DRM VC4 Grabber Diagnostic Test Script
# This script tests the grabber with Kodi running to identify stability issues

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="$SCRIPT_DIR/diagnostic_logs"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
TEST_LOG="$LOG_DIR/diagnostic_test_$TIMESTAMP.log"
GRABBER_LOG="$LOG_DIR/grabber_$TIMESTAMP.log"
SYSTEM_LOG="$LOG_DIR/system_$TIMESTAMP.log"

# Configuration
GRABBER_BINARY="$SCRIPT_DIR/target/release/drm-vc4-grabber"
TEST_DURATION=300  # 5 minutes default
MONITOR_INTERVAL=1000  # 1 second
HYPERION_ADDRESS="127.0.0.1:19400"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() {
    echo -e "[$(date '+%Y-%m-%d %H:%M:%S')] $1" | tee -a "$TEST_LOG"
}

log_error() {
    echo -e "[$(date '+%Y-%m-%d %H:%M:%S')] ${RED}ERROR: $1${NC}" | tee -a "$TEST_LOG"
}

log_warning() {
    echo -e "[$(date '+%Y-%m-%d %H:%M:%S')] ${YELLOW}WARNING: $1${NC}" | tee -a "$TEST_LOG"
}

log_success() {
    echo -e "[$(date '+%Y-%m-%d %H:%M:%S')] ${GREEN}SUCCESS: $1${NC}" | tee -a "$TEST_LOG"
}

log_info() {
    echo -e "[$(date '+%Y-%m-%d %H:%M:%S')] ${BLUE}INFO: $1${NC}" | tee -a "$TEST_LOG"
}

cleanup() {
    log "Cleaning up diagnostic test..."
    
    # Kill grabber if running
    if [ ! -z "$GRABBER_PID" ]; then
        log "Stopping grabber (PID: $GRABBER_PID)"
        kill -TERM "$GRABBER_PID" 2>/dev/null || true
        sleep 2
        kill -KILL "$GRABBER_PID" 2>/dev/null || true
    fi
    
    # Stop system monitoring
    if [ ! -z "$MONITOR_PID" ]; then
        log "Stopping system monitor (PID: $MONITOR_PID)"
        kill -TERM "$MONITOR_PID" 2>/dev/null || true
    fi
    
    log "Diagnostic test cleanup completed"
}

trap cleanup EXIT INT TERM

check_prerequisites() {
    log_info "Checking prerequisites..."
    
    # Create log directory
    mkdir -p "$LOG_DIR"
    
    # Check if grabber binary exists
    if [ ! -f "$GRABBER_BINARY" ]; then
        log_error "Grabber binary not found at $GRABBER_BINARY"
        log_info "Please build the project first: cargo build --release"
        exit 1
    fi
    
    # Check if running as root (needed for DRM access)
    if [ "$EUID" -ne 0 ]; then
        log_error "This script must be run as root for DRM access"
        log_info "Please run: sudo $0"
        exit 1
    fi
    
    # Check if Kodi is running
    if ! pgrep -f kodi > /dev/null; then
        log_warning "Kodi is not currently running"
        log_info "Starting Kodi for the test..."
        systemctl start kodi || log_warning "Failed to start Kodi via systemctl"
        sleep 5
        
        if ! pgrep -f kodi > /dev/null; then
            log_error "Could not start Kodi. Please start it manually and run this test again."
            exit 1
        fi
    fi
    
    log_success "Prerequisites check passed"
}

collect_initial_system_state() {
    log_info "Collecting initial system state..."
    
    {
        echo "=== INITIAL SYSTEM STATE ==="
        echo "Timestamp: $(date)"
        echo "Hostname: $(hostname)"
        echo "Kernel: $(uname -r)"
        echo "Uptime: $(uptime)"
        echo ""
        
        echo "=== MEMORY INFO ==="
        cat /proc/meminfo | head -10
        echo ""
        
        echo "=== KODI PROCESSES ==="
        ps aux | grep -E "(kodi|Kodi)" | grep -v grep
        echo ""
        
        echo "=== DRM CLIENTS (INITIAL) ==="
        cat /sys/kernel/debug/dri/0/clients 2>/dev/null || echo "DRM debug info not available"
        echo ""
        
        echo "=== GEM OBJECTS (INITIAL) ==="
        wc -l /sys/kernel/debug/dri/0/gem_names 2>/dev/null || echo "GEM debug info not available"
        echo ""
        
        echo "=== RECENT DMESG ==="
        dmesg -T | tail -20
        echo ""
        
    } > "$SYSTEM_LOG"
    
    log_success "Initial system state collected"
}

start_system_monitoring() {
    log_info "Starting continuous system monitoring..."
    
    {
        while true; do
            echo "=== MONITOR SNAPSHOT $(date) ==="
            
            # System load
            echo "Load: $(cat /proc/loadavg)"
            
            # Memory usage
            echo "Memory: $(free -h | grep Mem)"
            
            # Kodi status
            echo "Kodi PIDs: $(pgrep -f kodi | tr '\n' ' ')"
            
            # DRM clients
            echo "DRM clients: $(cat /sys/kernel/debug/dri/0/clients 2>/dev/null | wc -l || echo 'N/A')"
            
            # GEM objects
            echo "GEM objects: $(cat /sys/kernel/debug/dri/0/gem_names 2>/dev/null | wc -l || echo 'N/A')"
            
            # Check for recent errors
            RECENT_ERRORS=$(dmesg -T --since "30 seconds ago" 2>/dev/null | grep -E "(ERROR|WARN|drm|vc4|oom)" | wc -l)
            if [ "$RECENT_ERRORS" -gt 0 ]; then
                echo "ALERT: $RECENT_ERRORS recent kernel errors/warnings"
                dmesg -T --since "30 seconds ago" 2>/dev/null | grep -E "(ERROR|WARN|drm|vc4|oom)" | tail -5
            fi
            
            echo "---"
            sleep 5
        done
    } >> "$SYSTEM_LOG" &
    
    MONITOR_PID=$!
    log_success "System monitoring started (PID: $MONITOR_PID)"
}

run_grabber_test() {
    log_info "Starting grabber with diagnostic mode..."
    
    # Start the grabber with full diagnostic logging
    "$GRABBER_BINARY" \
        --diagnostic \
        --verbose \
        --log-file "$GRABBER_LOG" \
        --monitor-interval "$MONITOR_INTERVAL" \
        --address "$HYPERION_ADDRESS" &
    
    GRABBER_PID=$!
    log_success "Grabber started (PID: $GRABBER_PID)"
    
    # Monitor the grabber process
    log_info "Monitoring grabber for $TEST_DURATION seconds..."
    
    local start_time=$(date +%s)
    local end_time=$((start_time + TEST_DURATION))
    local last_check=0
    
    while [ $(date +%s) -lt $end_time ]; do
        # Check if grabber is still running
        if ! kill -0 "$GRABBER_PID" 2>/dev/null; then
            log_error "Grabber process died unexpectedly!"
            wait "$GRABBER_PID"
            local exit_code=$?
            log_error "Grabber exit code: $exit_code"
            return 1
        fi
        
        # Check if Kodi is still running
        if ! pgrep -f kodi > /dev/null; then
            log_error "Kodi process died during test!"
            return 1
        fi
        
        # Periodic status updates
        local current_time=$(date +%s)
        if [ $((current_time - last_check)) -ge 30 ]; then
            local elapsed=$((current_time - start_time))
            local remaining=$((TEST_DURATION - elapsed))
            log_info "Test progress: ${elapsed}s elapsed, ${remaining}s remaining"
            last_check=$current_time
        fi
        
        sleep 1
    done
    
    log_success "Grabber test completed successfully"
    return 0
}

analyze_results() {
    log_info "Analyzing test results..."
    
    {
        echo "=== DIAGNOSTIC TEST ANALYSIS ==="
        echo "Test completed at: $(date)"
        echo "Test duration: $TEST_DURATION seconds"
        echo ""
        
        echo "=== FINAL SYSTEM STATE ==="
        echo "Load: $(cat /proc/loadavg)"
        echo "Memory: $(free -h | grep Mem)"
        echo "Kodi status: $(pgrep -f kodi > /dev/null && echo 'RUNNING' || echo 'STOPPED')"
        echo ""
        
        echo "=== DRM STATE ANALYSIS ==="
        echo "Final DRM clients: $(cat /sys/kernel/debug/dri/0/clients 2>/dev/null | wc -l || echo 'N/A')"
        echo "Final GEM objects: $(cat /sys/kernel/debug/dri/0/gem_names 2>/dev/null | wc -l || echo 'N/A')"
        echo ""
        
        echo "=== ERROR SUMMARY ==="
        echo "Kernel errors during test:"
        dmesg -T --since "$TEST_DURATION seconds ago" 2>/dev/null | grep -E "(ERROR|WARN|drm|vc4|oom)" | wc -l || echo "0"
        
        echo ""
        echo "Recent kernel messages:"
        dmesg -T | tail -10
        echo ""
        
        if [ -f "$GRABBER_LOG" ]; then
            echo "=== GRABBER LOG SUMMARY ==="
            echo "Total log lines: $(wc -l < "$GRABBER_LOG")"
            echo "Errors in grabber log: $(grep -c "ERROR" "$GRABBER_LOG" 2>/dev/null || echo "0")"
            echo "Warnings in grabber log: $(grep -c "WARN" "$GRABBER_LOG" 2>/dev/null || echo "0")"
            echo ""
            echo "Last 10 grabber log entries:"
            tail -10 "$GRABBER_LOG"
        fi
        
    } >> "$TEST_LOG"
    
    log_success "Analysis completed"
}

print_summary() {
    echo ""
    echo "=========================================="
    echo "DIAGNOSTIC TEST SUMMARY"
    echo "=========================================="
    echo "Test logs saved to:"
    echo "  Main log: $TEST_LOG"
    echo "  Grabber log: $GRABBER_LOG"
    echo "  System log: $SYSTEM_LOG"
    echo ""
    echo "To analyze the results:"
    echo "  tail -f $GRABBER_LOG"
    echo "  grep ERROR $GRABBER_LOG"
    echo "  grep WARN $GRABBER_LOG"
    echo ""
    echo "To check for system issues:"
    echo "  dmesg -T | grep -E '(drm|vc4|oom)'"
    echo "  cat /sys/kernel/debug/dri/0/clients"
    echo "  cat /sys/kernel/debug/dri/0/gem_names | wc -l"
    echo "=========================================="
}

# Main execution
main() {
    log_info "Starting DRM VC4 Grabber diagnostic test"
    log_info "Test will run for $TEST_DURATION seconds"
    
    check_prerequisites
    collect_initial_system_state
    start_system_monitoring
    
    if run_grabber_test; then
        log_success "Test completed without crashes"
    else
        log_error "Test failed - system instability detected"
    fi
    
    analyze_results
    print_summary
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -d|--duration)
            TEST_DURATION="$2"
            shift 2
            ;;
        -a|--address)
            HYPERION_ADDRESS="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo "Options:"
            echo "  -d, --duration SECONDS    Test duration (default: 300)"
            echo "  -a, --address ADDRESS     Hyperion address (default: 127.0.0.1:19400)"
            echo "  -h, --help               Show this help"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

main "$@"