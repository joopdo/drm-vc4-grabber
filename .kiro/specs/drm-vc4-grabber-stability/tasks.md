# Implementation Plan

## Overview

This implementation plan addresses DRM VC4 Grabber stability through a phased approach that builds on existing diagnostic infrastructure. Analysis shows the current system has excellent monitoring capabilities, and the primary issues are network connectivity problems with Hyperion rather than fundamental DRM crashes. The plan prioritizes fixing actual stability issues first, then performance optimization, then advanced features.

## Phase Structure

**Phase 1: Core Stability (MANDATORY)** - Fix network reliability and validate DRM stability
**Phase 2: Performance Optimization** - Implement caching and memory optimizations  
**Phase 3: Advanced Features** - Add 4K support and advanced capabilities

Each phase has clear success criteria and must be completed before advancing to the next.

## Script Consolidation Guidelines

**CRITICAL: Minimize script proliferation**
- The model MUST consolidate functionality into existing scripts rather than creating new ones
- Only TWO main scripts are allowed: `build.sh` and `deploy.sh`
- New functionality MUST be added as options to existing scripts (e.g., `--diagnostic-test`, `--stability-test`)
- The model MUST NOT create temporary or specialized scripts without explicit user approval
- When implementing tasks, enhance existing scripts rather than creating new files

---

## Phase 1: Core Stability (MANDATORY)
*Success Criteria: Zero network failures + 24-hour stability test + Reliable LED operation*

### 1. Network Reliability and Connection Management

- [x] 1.1 Fix Hyperion connection reliability issues
  - ✅ Implement automatic reconnection on broken pipe errors
  - ✅ Add connection health monitoring and retry logic
  - ✅ Create graceful handling of Hyperion service restarts
  - ✅ Fixed sudo detection for LibreELEC (fake sudo always exits 1)
  - ✅ Removed unsafe static mutable references causing build warnings
  - _Requirements: 7.1, 7.5_

- [x] 1.2 Implement robust error recovery
  - ✅ Add exponential backoff for connection failures
  - ✅ Implement connection pooling for reliability
  - ✅ Create fallback modes when Hyperion is unavailable
  - ✅ Enhanced diagnostic logging with better debug output
  - ✅ Fixed deployment script issues with missing script references
  - _Requirements: 7.5, 6.5_

- [x] 1.3 Fix DRM resource management (CRITICAL - Kodi crashes) ✅ **MILESTONE ACHIEVED**
  - ✅ **COMPLETED**: Fixed GEM handle leaks in dump_framebuffer_to_image()
  - ✅ **COMPLETED**: Added comprehensive resource cleanup for all handles
  - ✅ **COMPLETED**: Integrated resource tracking with diagnostic logging
  - ✅ **COMPLETED**: Added periodic resource leak detection
  - ✅ **COMPLETED**: Improved cleanup to avoid duplicate handle warnings
  - **Root Cause**: Grabber was leaking GEM handles, causing Kodi DRM failures
  - **Evidence**: Kodi logs show "failed to convert prime fd to gem handle" errors
  - **Fix**: Clean up ALL GEM handles (not just handles[0]) after each capture
  - **RESULT**: ✅ Kodi now remains stable during video playback with grabber running
  - _Requirements: 6.1, 6.2, 6.3_

- [ ] 1.3 Add network diagnostics and monitoring
  - Monitor Hyperion service availability and response times
  - Log network error patterns and recovery success rates
  - Add network health reporting in diagnostic output
  - _Requirements: 2.1, 2.5_

- [ ]* 1.4 Write property test for network reliability
  - **Property 6: Network Recovery Consistency**
  - **Validates: Requirements 7.5, 6.5**

### 2. Comprehensive Diagnostic System Implementation

- [ ] 2.1 Implement thread-safe diagnostic logging
  - Create DiagnosticLogger with file-based output and timestamps
  - Implement configurable log categories and filtering
  - Add session boundary logging and system information capture
  - _Requirements: 2.1, 2.5_

- [ ] 2.2 Build resource tracking infrastructure
  - Implement ResourceTracker for Prime FD and GEM handle monitoring
  - Create resource lifecycle logging with allocation/deallocation tracking
  - Add resource leak detection and warning system
  - _Requirements: 2.2, 6.1, 6.2, 6.4_

- [ ]* 2.3 Write property test for resource conservation
  - **Property 1: Resource Conservation**
  - **Validates: Requirements 6.1, 6.3, 6.5**

- [ ] 2.4 Implement optimized logging output
  - Create configurable verbosity levels for different log categories
  - Implement milestone-based logging for routine operations
  - Add stdout filtering for critical events only
  - _Requirements: 2.3, 8.1, 8.4_

- [ ]* 2.5 Write property test for diagnostic completeness
  - **Property 2: Diagnostic Completeness**
  - **Validates: Requirements 2.2, 2.4, 2.5**

### 3. Real-time System Monitoring

- [ ] 3.1 Implement system health monitoring
  - Create SystemMonitor with configurable monitoring intervals
  - Add CPU load average and memory usage tracking
  - Implement memory pressure and OOM detection
  - _Requirements: 3.1, 3.3_

- [ ] 3.2 Build Kodi process monitoring
  - Implement Kodi process discovery and health tracking
  - Add memory usage and file descriptor monitoring for Kodi processes
  - Create process restart and crash detection
  - _Requirements: 3.1, 3.5_

- [ ] 3.3 Create DRM subsystem monitoring
  - Implement DRM client count tracking with change detection
  - Add GEM object statistics monitoring
  - Create DRM error detection from kernel messages
  - _Requirements: 3.2, 5.2, 5.3_

- [ ] 3.4 Implement adaptive monitoring frequency
  - Create reduced logging frequency for routine metrics
  - Add anomaly-based logging triggers
  - Implement monitoring cycle optimization
  - _Requirements: 3.4, 8.2, 8.3_

### 4. DRM Device Management and Selection

- [ ] 4.1 Implement proper DRM device selection
  - Create default selection of /dev/dri/card1 (VC4 driver)
  - Add DRM device capability verification
  - Implement driver information logging and validation
  - _Requirements: 5.1, 5.2, 5.4_

- [ ] 4.2 Build DRM authentication and initialization
  - Implement DRM device authentication with proper error handling
  - Add framebuffer capability verification
  - Create clear error messages for device selection failures
  - _Requirements: 5.3, 5.5_

- [ ]* 4.3 Write property test for concurrent DRM operation
  - **Property 3: Concurrent Operation Stability**
  - **Validates: Requirements 7.1, 7.2, 7.4**

### 5. Automated Stability Testing Framework

- [ ] 5.1 Create long-term stability testing infrastructure
  - Implement configurable test duration support (minutes to days)
  - Add automated grabber process monitoring and crash detection
  - Create test environment setup and validation
  - _Requirements: 4.1, 4.4_

- [ ] 5.2 Build comprehensive system monitoring during tests
  - Implement concurrent system resource logging
  - Add DRM kernel message capture and analysis
  - Create process health verification and reporting
  - _Requirements: 4.2, 4.4_

- [ ] 5.3 Implement automated test reporting
  - Create comprehensive test result analysis and reporting
  - Add resource leak detection and summary
  - Implement crash analysis and state preservation
  - _Requirements: 4.3, 10.2, 10.5_

- [ ] 5.4 Build video playback stress testing coordination
  - Create scripts for systematic video format testing
  - Add correlation analysis between playback patterns and stability
  - Implement crash reproduction capabilities
  - _Requirements: 4.2, 10.1, 10.3_

### 6. LibreELEC Environment Adaptations

- [ ] 6.1 Implement LibreELEC-specific filesystem handling
  - Create appropriate path handling for embedded filesystem layout
  - Add storage filesystem permission management
  - Implement LibreELEC system detection and adaptation
  - _Requirements: 9.2, 9.3_

- [ ] 6.2 Build static linking and compatibility system
  - Implement musl-based static binary generation
  - Add LibreELEC version compatibility verification
  - Create minimal dependency validation
  - _Requirements: 9.4, 9.5_

### 7. Error Handling and Recovery Systems

- [ ] 7.1 Implement comprehensive error categorization
  - Create error type classification (recoverable, resource, system, fatal)
  - Add appropriate error response strategies for each category
  - Implement graceful degradation for non-fatal errors
  - _Requirements: 2.5, 6.5, 7.5_

- [ ] 7.2 Build resource cleanup and recovery
  - Implement emergency cleanup procedures for fatal errors
  - Add resource verification and cleanup on process termination
  - Create recovery strategies for temporary failures
  - _Requirements: 6.3, 6.5_

### 8. Performance Optimization and Monitoring

- [ ] 8.1 Implement capture performance monitoring
  - Add frame capture latency measurement and logging
  - Create throughput sustainability monitoring
  - Implement performance regression detection
  - _Requirements: 7.4, 8.5_

- [ ]* 8.2 Write property test for log output optimization
  - **Property 5: Log Output Optimization**
  - **Validates: Requirements 8.1, 8.3, 8.4**

- [ ] 8.3 Build system impact measurement
  - Implement Kodi performance impact assessment
  - Add system responsiveness monitoring
  - Create resource usage proportionality verification
  - _Requirements: 7.1, 7.4_

### 9. Integration and Validation Testing

- [ ] 9.1 Create comprehensive integration test suite
  - Implement DRM multi-client interaction testing
  - Add Kodi coexistence validation scenarios
  - Create LibreELEC compatibility verification tests
  - _Requirements: 7.1, 7.2, 9.5_

- [ ]* 9.2 Write unit tests for diagnostic system components
  - Create tests for DiagnosticLogger thread safety and accuracy
  - Add ResourceTracker lifecycle verification tests
  - Implement SystemMonitor monitoring accuracy tests
  - _Requirements: 2.1, 2.2, 3.1_

- [ ]* 9.3 Write integration tests for build and deployment system
  - Create cross-compilation accuracy verification
  - Add deployment process validation tests
  - Implement LibreELEC environment compatibility tests
  - _Requirements: 1.1, 1.2, 9.1_

### 10. Final System Validation and Documentation

- [ ] 10.1 Conduct comprehensive stability validation
  - Execute 24+ hour stability tests with various video content
  - Validate resource leak prevention under extended operation
  - Verify crash reproduction and analysis capabilities
  - _Requirements: 4.1, 6.1, 10.1_

- [ ] 10.2 Create deployment and operational documentation
  - Document build system usage and troubleshooting
  - Create diagnostic log analysis guides
  - Implement stability testing procedures and best practices
  - _Requirements: 1.4, 2.1, 4.1_

- [ ] 10.3 Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.