# Requirements Document

## Introduction

The DRM VC4 Grabber Stability Analysis project addresses critical stability issues with a Rust-based screen capture application running on Raspberry Pi 5 with LibreELEC. The grabber captures framebuffer content via DRM (Direct Rendering Manager) and sends it to Hyperion for ambient lighting effects. The system experiences crashes, freezes, and resource leaks when running concurrently with Kodi during video playback, particularly with 4K content.

## Glossary

- **DRM_Grabber**: The Rust application that captures screen content via Direct Rendering Manager
- **LibreELEC**: Embedded Linux distribution optimized for Kodi media center
- **Kodi**: Open-source media player software
- **VC4_Driver**: Broadcom VideoCore IV display controller driver (/dev/dri/card1)
- **V3D_Driver**: Broadcom VideoCore 3D GPU driver (/dev/dri/card0)
- **Prime_FD**: File descriptor for sharing GPU memory between processes
- **GEM_Handle**: Graphics Execution Manager handle for GPU memory objects
- **Hyperion**: LED ambient lighting system that receives captured screen data
- **Diagnostic_System**: Comprehensive logging and monitoring infrastructure
- **Cross_Compilation**: Building ARM64 binaries on x86 development machines
- **Musl_Target**: Static linking target for LibreELEC compatibility

## Requirements

### Requirement 1

**User Story:** As a developer, I want a robust build and deployment system, so that I can rapidly iterate on stability fixes and deploy them to the target Raspberry Pi 5 system.

#### Acceptance Criteria

1. WHEN a developer runs the build script with cross-compilation, THE Build_System SHALL produce a statically-linked ARM64 binary compatible with LibreELEC
2. WHEN the deployment script is executed, THE Build_System SHALL automatically stop existing grabber processes on the target system
3. WHEN deploying to LibreELEC, THE Build_System SHALL detect the absence of sudo and adapt commands accordingly
4. WHEN the build process completes, THE Build_System SHALL provide clear instructions for testing on the target system
5. WHERE musl target is specified, THE Build_System SHALL create a fully static binary with no glibc dependencies

### Requirement 2

**User Story:** As a system administrator, I want comprehensive diagnostic logging, so that I can identify the root cause of stability issues and monitor system health during operation.

#### Acceptance Criteria

1. WHEN the grabber starts with diagnostic mode, THE Diagnostic_System SHALL create a local log file with timestamped entries
2. WHEN resource operations occur, THE Diagnostic_System SHALL track Prime FD and GEM handle allocation and deallocation
3. WHEN system anomalies are detected, THE Diagnostic_System SHALL log warnings for unusual DRM client counts or memory pressure
4. WHEN the grabber processes frames, THE Diagnostic_System SHALL log milestone events every 100 captures to track progress
5. WHEN errors occur, THE Diagnostic_System SHALL immediately log error details with full context

### Requirement 3

**User Story:** As a stability engineer, I want real-time system monitoring, so that I can observe the interaction between the grabber and Kodi during video playback stress testing.

#### Acceptance Criteria

1. WHEN system monitoring starts, THE System_Monitor SHALL track Kodi process health including memory usage and file descriptor counts
2. WHEN DRM subsystem state changes, THE System_Monitor SHALL log active client counts and GEM object statistics
3. WHEN memory pressure occurs, THE System_Monitor SHALL detect and log OOM killer activity and memory pressure indicators
4. WHILE monitoring is active, THE System_Monitor SHALL check system metrics at configurable intervals without overwhelming the log
5. WHEN Kodi processes change, THE System_Monitor SHALL detect process restarts or crashes

### Requirement 4

**User Story:** As a quality assurance engineer, I want automated stability testing tools, so that I can reproduce crashes under controlled conditions and validate fixes.

#### Acceptance Criteria

1. WHEN a stability test is initiated, THE Testing_System SHALL run the grabber for a specified duration while monitoring for crashes
2. WHEN video playback stress testing occurs, THE Testing_System SHALL coordinate with Kodi to play various video formats and resolutions
3. WHEN the test completes, THE Testing_System SHALL generate a comprehensive report showing resource leaks, errors, and system health
4. WHEN a crash is detected, THE Testing_System SHALL preserve diagnostic logs and system state for analysis
5. WHERE test duration is specified, THE Testing_System SHALL support configurable test periods from minutes to days

### Requirement 5

**User Story:** As a developer, I want proper DRM device selection, so that the grabber uses the correct display controller and avoids conflicts with GPU operations.

#### Acceptance Criteria

1. WHEN the grabber initializes, THE DRM_Grabber SHALL default to /dev/dri/card1 (VC4 display controller)
2. WHEN multiple DRM devices are available, THE DRM_Grabber SHALL provide clear logging about which device is selected
3. WHEN DRM authentication occurs, THE DRM_Grabber SHALL log driver information and capabilities
4. WHEN framebuffer capture begins, THE DRM_Grabber SHALL verify the selected device supports the required operations
5. WHERE device selection fails, THE DRM_Grabber SHALL provide actionable error messages

### Requirement 6

**User Story:** As a system operator, I want resource leak prevention, so that the grabber can run continuously without exhausting system resources or causing instability.

#### Acceptance Criteria

1. WHEN Prime FDs are allocated, THE Resource_Tracker SHALL record and monitor their lifecycle
2. WHEN GEM handles are created, THE Resource_Tracker SHALL ensure proper cleanup on process termination
3. WHEN capture operations complete, THE Resource_Tracker SHALL verify all temporary resources are released
4. WHEN resource leaks are detected, THE Resource_Tracker SHALL log detailed warnings with resource counts
5. WHERE the grabber terminates, THE Resource_Tracker SHALL perform cleanup verification and report any remaining resources

### Requirement 7

**User Story:** As a media center user, I want stable concurrent operation, so that ambient lighting works reliably during video playback without causing Kodi crashes or system freezes.

#### Acceptance Criteria

1. WHEN Kodi plays 4K video content, THE DRM_Grabber SHALL capture frames without interfering with video pipeline operations
2. WHEN multiple DRM clients are active, THE DRM_Grabber SHALL coordinate access to avoid resource conflicts
3. WHEN video format changes occur, THE DRM_Grabber SHALL adapt to new framebuffer configurations without crashing
4. WHEN system load increases, THE DRM_Grabber SHALL maintain stable operation without causing memory pressure
5. WHERE video playback errors occur in Kodi, THE DRM_Grabber SHALL continue operating independently

### Requirement 8

**User Story:** As a developer, I want optimized logging output, so that I can focus on critical events without being overwhelmed by routine operational data.

#### Acceptance Criteria

1. WHEN verbose logging is disabled, THE Diagnostic_System SHALL only output critical categories to stdout
2. WHEN system monitoring runs, THE Diagnostic_System SHALL reduce logging frequency for routine metrics
3. WHEN DRM client counts are stable, THE Diagnostic_System SHALL only log changes or anomalies
4. WHEN capture operations are successful, THE Diagnostic_System SHALL log milestones rather than every individual capture
5. WHERE log file size becomes large, THE Diagnostic_System SHALL maintain performance without blocking operations

### Requirement 9

**User Story:** As a deployment engineer, I want LibreELEC-specific adaptations, so that the grabber works seamlessly in the embedded environment with its unique constraints.

#### Acceptance Criteria

1. WHEN deploying to LibreELEC, THE Deployment_System SHALL detect the absence of sudo and use direct root commands
2. WHEN creating directories on LibreELEC, THE Deployment_System SHALL ensure proper permissions for the storage filesystem
3. WHEN the grabber runs on LibreELEC, THE DRM_Grabber SHALL use appropriate paths for the embedded filesystem layout
4. WHEN static linking is required, THE Build_System SHALL produce musl-based binaries compatible with LibreELEC's minimal environment
5. WHERE LibreELEC updates occur, THE DRM_Grabber SHALL maintain compatibility across system versions

### Requirement 10

**User Story:** As a troubleshooting engineer, I want crash reproduction capabilities, so that I can systematically identify the conditions that trigger stability issues.

#### Acceptance Criteria

1. WHEN crash testing begins, THE Testing_System SHALL provide scripts to stress test with various video formats
2. WHEN crashes occur, THE Testing_System SHALL preserve the exact system state and diagnostic logs leading to the failure
3. WHEN video playback patterns change, THE Testing_System SHALL monitor for correlation with grabber stability
4. WHEN resource exhaustion approaches, THE Testing_System SHALL detect early warning signs before system failure
5. WHERE crashes are reproduced, THE Testing_System SHALL provide detailed analysis of the failure sequence