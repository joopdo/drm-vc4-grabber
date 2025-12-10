#[macro_use]
extern crate nix;

use std::fs::{File, OpenOptions};

use std::os::fd::AsFd;

use clap::{App, Arg};
use drm::control::framebuffer::Handle;
use drm::control::Device as ControlDevice;
use drm::Device;
use drm_ffi::drm_set_client_cap;

use dump_image::dump_framebuffer_to_image;
use image::{ImageError, RgbImage};

use std::os::unix::io::{AsRawFd, RawFd};
use std::{thread, time::Duration};

use std::io::Result as StdResult;

pub mod ffi;
pub mod framebuffer;
pub mod hyperion;
pub mod hyperion_reply_generated;
pub mod hyperion_request_generated;
pub mod image_decoder;
pub mod dump_image;
pub mod diagnostics;
pub mod system_monitor;
pub mod connection_manager;

pub use hyperion_request_generated::hyperionnet::{Clear, Color, Command, Image, Register};


use diagnostics::{DiagnosticLogger, ResourceTracker};
use system_monitor::SystemMonitor;
use connection_manager::{HyperionConnectionManager, ConnectionConfig};
use std::sync::Arc;

pub struct Card(File);

impl AsRawFd for Card {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl AsFd for Card {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl Device for Card {}
impl ControlDevice for Card {}

impl Card {
    pub fn open(path: &str) -> Self {
        let mut options = OpenOptions::new();
        options.read(true);
        options.write(false);
        Card(options.open(path).unwrap())
    }
}

fn save_screenshot(img: &RgbImage) -> Result<(), ImageError> {
    img.save("screenshot.png")
}

fn send_dumped_image_with_manager(
    connection_manager: &mut HyperionConnectionManager, 
    img: &RgbImage, 
    verbose: bool,
    _logger: Option<&DiagnosticLogger>
) -> Result<bool, std::io::Error> {
    connection_manager.send_image_with_fallback(img, verbose)
}

fn dump_and_send_framebuffer(
    connection_manager: &mut HyperionConnectionManager,
    card: &Card,
    fb: Handle,
    verbose: bool,
    logger: Option<&DiagnosticLogger>,
    resource_tracker: Option<&ResourceTracker>,
) -> StdResult<()> {
    // Only log capture start in verbose mode or buffer it
    if let Some(logger) = logger {
        if verbose {
            logger.log_immediate("CAPTURE", &format!("Starting framebuffer capture for handle {:?}", fb));
        } else {
            logger.log_buffered("CAPTURE", &format!("Starting framebuffer capture for handle {:?}", fb));
        }
    }
    
    // Track resource state before capture
    if let Some(tracker) = resource_tracker {
        tracker.log_current_state();
    }
    
    let img = dump_framebuffer_to_image(card, fb, verbose);
    match img {
        Ok(img) => {
            if let Some(logger) = logger {
                // Buffer successful captures, only log immediately if verbose
                if verbose {
                    logger.log_immediate("CAPTURE", &format!("Successfully captured {}x{} image", img.width(), img.height()));
                } else {
                    logger.log_buffered("CAPTURE", &format!("Successfully captured {}x{} image", img.width(), img.height()));
                }
                
                // Count successful captures for periodic summaries
                logger.log_capture_success();
            }
            
            // Track resource state after successful capture
            if let Some(tracker) = resource_tracker {
                if verbose {
                    logger.map(|l| l.log("RESOURCE", "Post-capture resource check"));
                }
                tracker.log_current_state();
            }
            
            match send_dumped_image_with_manager(connection_manager, &img, verbose, logger) {
                Ok(sent) => {
                    if !sent && logger.is_some() {
                        // Operating in fallback mode or temporary failure
                        if connection_manager.is_failed() {
                            // Log fallback mode less frequently to avoid spam
                            use std::sync::Mutex;
                            use std::sync::OnceLock;
                            static LAST_FALLBACK_LOG: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();
                            let last_log = LAST_FALLBACK_LOG.get_or_init(|| Mutex::new(None));
                            let now = std::time::Instant::now();
                            
                            if let Ok(mut last) = last_log.lock() {
                                if last.is_none() || 
                                   last.unwrap().elapsed() >= Duration::from_secs(60) {
                                    if let Some(logger) = logger {
                                        logger.log("FALLBACK", "Operating without Hyperion - capture continues");
                                    }
                                    *last = Some(now);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Err(e) => {
            let error_msg = format!("Error dumping framebuffer to image: {}", e);
            if let Some(logger) = logger {
                logger.log_capture_error(&error_msg);
            } else {
                println!("{}", error_msg);
            }
            
            // Track resource state after error to detect leaks
            if let Some(tracker) = resource_tracker {
                logger.map(|l| l.log_warning("Checking for resource leaks after capture error"));
                tracker.check_for_leaks();
            }
        }
    }

    Ok(())
}

fn find_framebuffer(card: &Card, verbose: bool) -> Option<Handle> {
    let resource_handles = card.resource_handles().unwrap();

    for crtc in resource_handles.crtcs() {
        let info = card.get_crtc(*crtc).unwrap();

        if verbose {
            println!("CRTC Info: {:?}", info);
        }

        if info.mode().is_some() {
            if let Some(fb) = info.framebuffer() {
                return Some(fb);
            }
        }
    }

    let plane_handles = card.plane_handles().unwrap();

    for plane in plane_handles.planes() {
        let info = card.get_plane(*plane).unwrap();

        if verbose {
            println!("Plane Info: {:?}", info);
        }

        if info.crtc().is_some() {
            let fb = info.framebuffer().unwrap();

            return Some(fb);
        }
    }

    None
}

fn main() {
    let matches = App::new("DRM VC4 Screen Grabber for Hyperion")
        .version("0.1.0")
        .author("Rudi Horn <dyn-git@rudi-horn.de>")
        .about("Captures a screenshot and sends it to the Hyperion server.")
        .arg(
            Arg::with_name("device")
                .short("d")
                .long("device")
                .default_value("/dev/dri/card1")
                .takes_value(true)
                .help("The device path of the DRM device to capture the image from."),
        )
        .arg(
            Arg::with_name("address")
                .short("a")
                .long("address")
                .default_value("127.0.0.1:19400")
                .takes_value(true)
                .help("The Hyperion TCP socket address to send the captured screenshots to."),
        )
        .arg(
            Arg::with_name("screenshot")
                .long("screenshot")
                .takes_value(false)
                .help("Capture a screenshot and save it to screenshot.png"),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .help("Print verbose debugging information."),
        )
        .arg(
            Arg::with_name("diagnostic")
                .long("diagnostic")
                .takes_value(false)
                .help("Enable comprehensive diagnostic mode with system monitoring"),
        )
        .arg(
            Arg::with_name("log-file")
                .long("log-file")
                .default_value("drm-grabber-diagnostic.log")
                .takes_value(true)
                .help("Path to diagnostic log file"),
        )
        .arg(
            Arg::with_name("monitor-interval")
                .long("monitor-interval")
                .default_value("1000")
                .takes_value(true)
                .help("System monitoring interval in milliseconds"),
        )
        .arg(
            Arg::with_name("fps")
                .long("fps")
                .default_value("20")
                .takes_value(true)
                .help("Target frames per second for capture"),
        )
        .arg(
            Arg::with_name("max-retries")
                .long("max-retries")
                .default_value("10")
                .takes_value(true)
                .help("Maximum connection retry attempts before entering fallback mode"),
        )
        .arg(
            Arg::with_name("connection-timeout")
                .long("connection-timeout")
                .default_value("3000")
                .takes_value(true)
                .help("Connection timeout in milliseconds"),
        )
        .get_matches();

    let verbose = matches.is_present("verbose");
    let screenshot = matches.is_present("screenshot");
    let diagnostic_mode = matches.is_present("diagnostic");
    let log_file = matches.value_of("log-file").unwrap();
    let monitor_interval: u64 = matches.value_of("monitor-interval")
        .unwrap()
        .parse()
        .expect("Invalid monitor interval");
    let fps: u64 = matches.value_of("fps")
        .unwrap()
        .parse()
        .expect("Invalid FPS value");
    let max_retries: u32 = matches.value_of("max-retries")
        .unwrap()
        .parse()
        .expect("Invalid max-retries value");
    let connection_timeout: u64 = matches.value_of("connection-timeout")
        .unwrap()
        .parse()
        .expect("Invalid connection-timeout value");
    
    // Initialize diagnostic logging
    let logger = if diagnostic_mode || verbose {
        println!("Initializing diagnostic logging to: {}", log_file);
        match DiagnosticLogger::new(log_file) {
            Ok(logger) => {
                let logger = Arc::new(logger);
                logger.log("INIT", "Diagnostic logging initialized");
                logger.log("INIT", &format!("Command line args: diagnostic={}, verbose={}, fps={}, max_retries={}", 
                                           diagnostic_mode, verbose, fps, max_retries));
                println!("Diagnostic logging successfully initialized");
                Some(logger)
            }
            Err(e) => {
                eprintln!("Failed to initialize diagnostic logging: {}", e);
                None
            }
        }
    } else {
        println!("Diagnostic mode not enabled - use --diagnostic or --verbose to enable logging");
        None
    };
    
    let device_path = matches.value_of("device").unwrap();
    
    if let Some(ref logger) = logger {
        logger.log("INIT", &format!("Opening DRM device: {}", device_path));
    }
    
    let card = Card::open(device_path);
    let authenticated = card.authenticated().unwrap();

    if verbose || logger.is_some() {
        let driver = card.get_driver().unwrap();
        let msg = format!("Driver (auth={}): {:?}", authenticated, driver);
        if let Some(ref logger) = logger {
            logger.log("DRM", &msg);
        } else {
            println!("{}", msg);
        }
    }
    
    // Initialize resource tracking if diagnostic mode is enabled
    let resource_tracker = logger.as_ref().map(|l| Arc::new(ResourceTracker::new(Arc::clone(l))));
    
    // Track initial system state
    if let Some(ref logger) = logger {
        logger.log("INIT", "Checking initial system resource state");
        if let Some(ref tracker) = resource_tracker {
            tracker.log_current_state();
        }
    }
    
    // Start system monitoring if diagnostic mode is enabled
    let _system_monitor = if diagnostic_mode {
        if let Some(ref logger) = logger {
            let monitor = SystemMonitor::new(Arc::clone(logger));
            monitor.start_monitoring(monitor_interval);
            Some(monitor)
        } else {
            None
        }
    } else {
        None
    };

    unsafe {
        let set_cap = drm_set_client_cap{ capability: drm_ffi::DRM_CLIENT_CAP_UNIVERSAL_PLANES as u64, value: 1 };
        drm_ffi::ioctl::set_cap(card.as_raw_fd(), &set_cap).unwrap();
    }

    let address = matches.value_of("address").unwrap();
    if screenshot {
        if let Some(fb) = find_framebuffer(&card, verbose) {
            let img = dump_framebuffer_to_image(&card, fb, verbose).unwrap();
            save_screenshot(&img).unwrap();
        } else {
            println!("No framebuffer found!");
        }
    } else {
        // Initialize connection manager with robust retry logic
        let connection_config = ConnectionConfig {
            address: address.to_string(),
            max_retries,
            initial_backoff_ms: 100,
            max_backoff_ms: 5000,
            connection_timeout_ms: connection_timeout,
            health_check_interval_ms: 30000,
        };
        
        println!("Creating connection manager for {} with {} max retries", address, max_retries);
        if let Some(ref logger) = logger {
            logger.log("INIT", &format!("Creating connection manager for {} with {} max retries", address, max_retries));
        }
        
        let mut connection_manager = HyperionConnectionManager::new(
            connection_config, 
            logger.clone()
        );
        
        // Initial connection attempt
        if let Err(e) = connection_manager.ensure_connected() {
            if let Some(ref logger) = logger {
                logger.log_error(&format!("Failed to establish initial Hyperion connection: {}", e));
            } else {
                eprintln!("Failed to establish initial Hyperion connection: {}", e);
            }
        }

        let mut frame_count = 0u64;
        loop {
            if let Some(fb) = find_framebuffer(&card, verbose) {
                let result = dump_and_send_framebuffer(
                    &mut connection_manager,
                    &card, 
                    fb, 
                    verbose,
                    logger.as_ref().map(|l| l.as_ref()),
                    resource_tracker.as_ref().map(|t| t.as_ref())
                );
                
                if let Err(e) = result {
                    if let Some(ref logger) = logger {
                        logger.log_error(&format!("Main loop error: {}", e));
                    } else {
                        eprintln!("Main loop error: {}", e);
                    }
                    
                    // Check for resource leaks after errors
                    if let Some(ref tracker) = resource_tracker {
                        tracker.check_for_leaks();
                    }
                    
                    // Add a small delay on errors to prevent tight error loops
                    thread::sleep(Duration::from_millis(100));
                }
                
                frame_count += 1;
                
                // Periodic resource leak check (every 100 frames)
                if diagnostic_mode && frame_count % 100 == 0 {
                    if let Some(ref tracker) = resource_tracker {
                        if tracker.check_for_leaks() {
                            if let Some(ref logger) = logger {
                                logger.log_warning(&format!("Resource leak detected after {} frames", frame_count));
                            }
                        }
                    }
                }
                
                thread::sleep(Duration::from_millis(1000/fps));
            } else {
                if let Some(ref logger) = logger {
                    logger.log_warning("No framebuffer found, waiting...");
                }
                thread::sleep(Duration::from_secs(1));
            }
            
            // Periodically log connection statistics and check for service recovery
            if diagnostic_mode && logger.is_some() {
                use std::sync::Mutex;
                use std::sync::OnceLock;
                static LAST_STATS_LOG: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();
                static LAST_RECOVERY_CHECK: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();
                let stats_log = LAST_STATS_LOG.get_or_init(|| Mutex::new(None));
                let recovery_check = LAST_RECOVERY_CHECK.get_or_init(|| Mutex::new(None));
                let now = std::time::Instant::now();
                
                // Log stats every 5 minutes
                if let Ok(mut last_stats) = stats_log.lock() {
                    if last_stats.is_none() || 
                       last_stats.unwrap().elapsed() >= Duration::from_secs(300) {
                        let stats = connection_manager.get_stats();
                        if let Some(ref logger) = logger {
                            logger.log("HYPERION_STATS", &format!(
                                "Connection stats - State: {}, Failures: {}, Reconnections: {}, Uptime: {}s",
                                stats.state, stats.consecutive_failures, stats.total_reconnections, stats.uptime_seconds
                            ));
                        }
                        *last_stats = Some(now);
                    }
                }
                
                // Check for service recovery every 2 minutes if in failed state
                if connection_manager.is_failed() {
                    if let Ok(mut last_recovery) = recovery_check.lock() {
                        if last_recovery.is_none() || 
                           last_recovery.unwrap().elapsed() >= Duration::from_secs(120) {
                            if let Some(ref logger) = logger {
                                logger.log("HYPERION", "Checking if Hyperion service has recovered...");
                            }
                            connection_manager.reset_failure_state();
                            *last_recovery = Some(now);
                        }
                    }
                }
            }
        }
    }
}
