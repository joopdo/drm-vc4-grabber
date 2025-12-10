# DRM VC4 Grabber Diagnostic Guide

This guide explains how to use the diagnostic features to troubleshoot stability issues with the DRM VC4 Grabber on Raspberry Pi 5.

## Overview

The diagnostic system provides comprehensive logging and monitoring to help identify the root cause of system instability when running the grabber alongside Kodi.

## Features

### 1. Diagnostic Logging
- **File-based logging**: All events are logged to a file for analysis
- **Categorized messages**: Different types of events (DRM, HYPERION, ERROR, etc.)
- **Timestamps**: Both absolute and relative timestamps for correlation
- **Resource tracking**: Monitor Prime FDs and GEM handles

### 2. System Monitoring
- **Real-time monitoring**: Continuous system state monitoring
- **Kodi integration**: Track Kodi process status and resource usage
- **DRM subsystem**: Monitor DRM clients and GEM objects
- **Memory pressure**: Detect OOM conditions and memory issues

### 3. Automated Testing
- **Comprehensive test suite**: Automated stability testing
- **System state collection**: Before/after system state comparison
- **Error detection**: Automatic detection of crashes and errors

## Quick Start

### 1. Build with Diagnostic Support

```bash
# Build the project
./build.sh

# Or manually:
cargo build --release
```

### 2. Run Diagnostic Test

The easiest way to test stability is using the automated diagnostic script:

```bash
# Run 5-minute stability test (requires root)
sudo ./diagnostic_test.sh

# Run longer test (30 minutes)
sudo ./diagnostic_test.sh --duration 1800

# Test with different Hyperion address
sudo ./diagnostic_test.sh --address 192.168.1.100:19400
```

### 3. Manual Diagnostic Mode

For manual testing with full diagnostic logging:

```bash
# Run with diagnostic mode enabled
sudo ./target/release/drm-vc4-grabber --diagnostic --verbose

# Custom log file and monitoring interval
sudo ./target/release/drm-vc4-grabber \
    --diagnostic \
    --log-file /tmp/grabber-debug.log \
    --monitor-interval 500
```

## Understanding the Logs

### Log Categories

- **SESSION**: Session start/end markers
- **SYSTEM**: System information and state
- **DRM**: DRM operations and resource management
- **FB**: Framebuffer information
- **HYPERION**: Hyperion communication
- **CAPTURE**: Image capture operations
- **TRACK**: Resource allocation/deallocation tracking
- **MONITOR**: System monitoring data
- **ERROR**: Error conditions
- **WARN**: Warning conditions

### Example Log Analysis

```bash
# Check for errors
grep ERROR drm-grabber-diagnostic.log

# Monitor resource leaks
grep "TRACK.*allocated" drm-grabber-diagnostic.log | wc -l
grep "TRACK.*released" drm-grabber-diagnostic.log | wc -l

# Check DRM operations
grep "DRM" drm-grabber-diagnostic.log

# Monitor system state
grep "MONITOR" drm-grabber-diagnostic.log | tail -20
```

## Common Issues and Diagnosis

### 1. System Freezes

**Symptoms:**
- System becomes unresponsive
- SSH connections drop
- Kodi stops responding

**Diagnostic Steps:**
```bash
# Check for kernel panics
dmesg -T | grep -E "(panic|oops|BUG)"

# Monitor DRM clients before crash
watch -n 1 "cat /sys/kernel/debug/dri/0/clients"

# Check memory pressure
cat /proc/pressure/memory
```

### 2. Kodi Crashes

**Symptoms:**
- Kodi restarts unexpectedly
- "DRM-GRABBER STOP" messages
- Video playback interruptions

**Diagnostic Steps:**
```bash
# Check Kodi logs
tail -f /var/log/kodi.log

# Monitor Kodi process
watch -n 1 "ps aux | grep kodi"

# Check for DRM conflicts
grep "drm" /var/log/syslog
```

### 3. Resource Leaks

**Symptoms:**
- Increasing memory usage over time
- High file descriptor count
- GEM object accumulation

**Diagnostic Steps:**
```bash
# Monitor GEM objects
watch -n 1 "cat /sys/kernel/debug/dri/0/gem_names | wc -l"

# Check file descriptors
ls /proc/$(pgrep drm-vc4-grabber)/fd | wc -l

# Monitor memory usage
watch -n 1 "cat /proc/$(pgrep drm-vc4-grabber)/status | grep VmRSS"
```

## Advanced Debugging

### 1. Kernel Tracing

Enable DRM tracing for detailed kernel-level debugging:

```bash
# Enable DRM tracing
echo 1 > /sys/kernel/debug/tracing/events/drm/enable

# Monitor DRM events
cat /sys/kernel/debug/tracing/trace_pipe | grep drm
```

### 2. Performance Profiling

Use perf to analyze performance bottlenecks:

```bash
# Profile the grabber
sudo perf record -g ./target/release/drm-vc4-grabber --diagnostic

# Analyze results
sudo perf report
```

### 3. Memory Analysis

Use valgrind for memory leak detection (debug build required):

```bash
# Build debug version
cargo build

# Run with valgrind
sudo valgrind --leak-check=full ./target/debug/drm-vc4-grabber --screenshot
```

## Interpreting Test Results

### Successful Test
- No system freezes during test duration
- Kodi remains responsive
- No kernel errors in dmesg
- Resource counts remain stable

### Failed Test Indicators
- System becomes unresponsive
- Kodi process disappears
- Kernel errors/warnings in dmesg
- Increasing resource counts
- OOM killer activation

## Next Steps

Based on diagnostic results:

1. **If stable without Kodi**: The issue is DRM access conflict
2. **If unstable alone**: Hardware or driver issue
3. **If resource leaks detected**: Memory management problem
4. **If kernel errors present**: Low-level DRM driver issue

## Reporting Issues

When reporting stability issues, please include:

1. Complete diagnostic log file
2. System monitoring output
3. dmesg output during the issue
4. Hardware information (Pi model, OS version)
5. Kodi version and configuration

## Configuration Files

The diagnostic system creates several log files:

- `drm-grabber-diagnostic.log`: Main diagnostic log
- `diagnostic_logs/`: Directory for test logs
- `diagnostic_test_TIMESTAMP.log`: Test execution log
- `system_TIMESTAMP.log`: System monitoring log
- `grabber_TIMESTAMP.log`: Grabber-specific log

Keep these files for analysis and issue reporting.