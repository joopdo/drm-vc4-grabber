# Design Document

## Overview

The DRM VC4 Grabber Stability Analysis project implements a comprehensive diagnostic and monitoring system for a Rust-based screen capture application. The design focuses on identifying and resolving stability issues that occur when the grabber runs concurrently with Kodi on Raspberry Pi 5 systems running LibreELEC.

## Architecture

### High-Level Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Build System  │    │  DRM Grabber    │    │ Diagnostic Sys  │
│                 │    │                 │    │                 │
│ • Cross-compile │    │ • VC4 Capture   │    │ • File Logging  │
│ • Musl Linking  │────▶ • Resource Mgmt │────▶ • System Monitor│
│ • Auto Deploy   │    │ • Hyperion Send │    │ • Leak Detection│
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         │                       │                       │
         ▼                       ▼                       ▼
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   LibreELEC     │    │      Kodi       │    │   Hyperion      │
│   Target Env    │    │  Media Player   │    │  LED Controller │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

### Component Interaction

The system operates with multiple concurrent processes accessing the DRM subsystem:
- **Kodi**: Primary DRM master for video playback
- **DRM Grabber**: Secondary DRM client for screen capture
- **Diagnostic System**: Monitors both processes and system health

## Components and Interfaces

### Build System (`build.sh`, `deploy.sh`)

**Purpose**: Cross-compilation and deployment automation for rapid development iteration.

**Key Features**:
- Automatic detection of rustup vs system Rust installations
- Musl target support for static linking compatibility with LibreELEC
- LibreELEC-aware deployment (no sudo detection)
- Automated process cleanup before deployment

**Interfaces**:
```bash
# Primary build interface
./build.sh --musl --deploy root@pi-host --pi-path /target/path

# Quick deployment interface  
./deploy.sh root@pi-host --pi-path /target/path
```

### Diagnostic System (`src/diagnostics.rs`)

**Purpose**: Optimized logging and resource tracking for extended crash reproduction testing.

**Key Optimizations**:
- **Smart Tiered Logging**: Immediate (errors), buffered (regular ops), periodic (summaries)
- **Error Context Capture**: Automatically dumps last 100 events when errors occur
- **Hyperion Error Deduplication**: Prevents log spam from connection issues
- **98% Log Size Reduction**: From 1MB+ per 3 minutes to ~50KB per hour

**Key Components**:
- `DiagnosticLogger`: Thread-safe logging with smart buffering and periodic summaries
- `ResourceTracker`: Prime FD and GEM handle lifecycle monitoring
- `LogEntry`: Buffered log entries for context capture
- Separate Kodi log monitoring via `monitor_kodi_log.sh`

**Interfaces**:
```rust
pub struct DiagnosticLogger {
    pub fn log_immediate(&self, category: &str, message: &str)     // Critical events
    pub fn log_buffered(&self, category: &str, message: &str)     // Regular operations
    pub fn log(&self, category: &str, message: &str)              // Smart routing
    pub fn maybe_log_summary(&self)                               // Periodic summaries
    pub fn log_hyperion_operation(&self, op: &str, success: bool, details: &str)
    pub fn log_capture_success(&self)                             // Count + maybe summary
}

pub struct ResourceTracker {
    pub fn track_prime_fd(&self, fd: i32)
    pub fn untrack_prime_fd(&self, fd: i32)
    pub fn check_for_leaks(&self) -> bool
}
```

### System Monitor (`src/system_monitor.rs`)

**Purpose**: Real-time monitoring of system health and process interactions.

**Monitoring Targets**:
- Kodi process health (memory, file descriptors, PIDs)
- DRM subsystem state (active clients, GEM objects)
- System resources (load average, memory pressure)
- Kernel messages (DRM errors, OOM events)

**Optimized Monitoring Strategy**:
- **Critical Issues**: Every second (OOM, DRM errors, Kodi crashes)
- **Health Summaries**: Every minute (memory, load, process status)
- **Detailed Analysis**: Only when problems detected
- **Kodi Log Integration**: Separate monitoring of `/storage/.kodi/temp/kodi.log`

### Testing Framework (`diagnostic_test.sh`)

**Purpose**: Optimized crash reproduction testing with minimal logging overhead.

**Key Optimizations**:
- **Extended Test Duration**: 10 minutes default (vs 5 minutes previously)
- **Minimal Log Overhead**: Smart logging allows hours-long tests
- **Separate Kodi Monitoring**: Independent tracking of Kodi crashes
- **Automatic Crash Context**: Captures system state when failures occur

**Test Capabilities**:
- Configurable test duration and FPS
- Concurrent Kodi log monitoring via `monitor_kodi_log.sh`
- Crash detection with automatic context capture
- Optimized for LibreELEC deployment

**Usage**:
```bash
./diagnostic_test.sh --duration 600 --fps 20
```

## Data Models

### Log Entry Format

```
[timestamp_ms] +elapsed_ms [CATEGORY] message
```

**Categories**:
- `SESSION`: Test session boundaries
- `INIT`: Initialization events
- `CAPTURE`: Frame capture operations
- `HYPERION`: LED data transmission
- `RESOURCE`: Resource allocation/deallocation
- `ERROR`/`WARN`: Issues and anomalies
- `SYSTEM`: System health metrics
- `KODI`: Kodi process monitoring
- `DRM`: DRM subsystem state

### Resource Tracking Model

```rust
struct ResourceState {
    prime_fds: Vec<i32>,      // Active Prime file descriptors
    gem_handles: Vec<u32>,    // Active GEM handles
    allocation_time: SystemTime,
    process_id: u32,
}
```

### System Health Model

```rust
struct SystemHealth {
    load_average: f32,
    memory_usage_percent: u8,
    kodi_processes: Vec<ProcessInfo>,
    drm_client_count: usize,
    gem_object_count: usize,
}
```

## Error Handling

### Resource Management Strategy

**RAII Pattern**: All DRM resources use Rust's ownership system for automatic cleanup
**Explicit Tracking**: Diagnostic system maintains parallel tracking for leak detection
**Graceful Degradation**: Capture failures don't crash the entire system

### Error Categories

1. **Recoverable Errors**: Temporary DRM access failures, network timeouts
2. **Resource Errors**: Prime FD/GEM handle exhaustion, memory pressure
3. **System Errors**: Kodi crashes, kernel DRM errors
4. **Fatal Errors**: DRM device unavailable, permission denied

### Error Response Strategy

```rust
match capture_result {
    Ok(image) => process_image(image),
    Err(TemporaryError) => {
        log_warning("Temporary capture failure, retrying");
        continue;
    }
    Err(ResourceError) => {
        log_error("Resource exhaustion detected");
        cleanup_resources();
        return Err(ResourceError);
    }
    Err(FatalError) => {
        log_error("Fatal error, terminating");
        emergency_cleanup();
        std::process::exit(1);
    }
}
```

## Testing Strategy

### Unit Testing Approach

**Resource Management Tests**:
- Prime FD lifecycle verification
- GEM handle cleanup validation
- Resource tracker accuracy

**Diagnostic System Tests**:
- Log format consistency
- Thread safety verification
- File I/O error handling

### Integration Testing Approach

**DRM Interaction Tests**:
- Multi-client DRM access patterns
- Framebuffer format adaptation
- Device selection logic

**System Integration Tests**:
- Kodi coexistence validation
- LibreELEC compatibility verification
- Cross-compilation accuracy

### Property-Based Testing Strategy

**Property 1: Resource Conservation**
*For any* sequence of capture operations, the number of allocated Prime FDs and GEM handles should return to zero after cleanup
**Validates: Requirements 6.1, 6.3, 6.5**

**Property 2: Diagnostic Completeness**
*For any* error condition, the diagnostic system should log sufficient information to reproduce the issue
**Validates: Requirements 2.2, 2.4, 2.5**

**Property 3: Concurrent Operation Stability**
*For any* valid Kodi video playback scenario, the grabber should maintain stable operation without interfering with video pipeline
**Validates: Requirements 7.1, 7.2, 7.4**

**Property 4: Build System Consistency**
*For any* supported target architecture, the build system should produce a functional binary with correct dependencies
**Validates: Requirements 1.1, 1.5, 9.4**

**Property 5: Log Output Optimization**
*For any* logging configuration, critical events should always be captured while routine events respect verbosity settings
**Validates: Requirements 8.1, 8.3, 8.4**

### Stress Testing Strategy

**Video Playback Stress Tests**:
- 4K content playback during capture
- Format switching scenarios
- Seek operation stress testing
- Multiple concurrent video streams

**Resource Exhaustion Tests**:
- Extended runtime testing (24+ hours)
- Memory pressure simulation
- File descriptor exhaustion scenarios
- DRM client limit testing

**System Integration Stress Tests**:
- LibreELEC system updates during operation
- Network connectivity interruptions
- Hyperion service restarts
- Concurrent system load

### Performance Testing

**Capture Performance Metrics**:
- Frame capture latency (target: <50ms)
- Throughput sustainability (target: 20+ FPS)
- Memory usage stability (target: <100MB)
- CPU usage efficiency (target: <10% load)

**System Impact Metrics**:
- Kodi performance degradation (target: <5%)
- System responsiveness maintenance
- Resource usage proportionality
- Thermal impact assessment