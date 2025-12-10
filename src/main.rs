#[macro_use]
extern crate nix;

use std::fs::{File, OpenOptions};
use std::net::TcpStream;
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

pub use hyperion_request_generated::hyperionnet::{Clear, Color, Command, Image, Register};

use hyperion::{read_reply, register_direct, send_color_red, send_image};
use diagnostics::{DiagnosticLogger, ResourceTracker};
use system_monitor::SystemMonitor;
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

fn send_dumped_image(socket: &mut TcpStream, img: &RgbImage, verbose : bool) -> StdResult<()> {
    register_direct(socket)?;
    read_reply(socket, verbose)?;

    send_image(socket, img, verbose)?;

    Ok(())
}

fn dump_and_send_framebuffer(
    socket: &mut TcpStream,
    card: &Card,
    fb: Handle,
    verbose: bool,
    logger: Option<&DiagnosticLogger>,
    resource_tracker: Option<&ResourceTracker>,
) -> StdResult<()> {
    if let Some(logger) = logger {
        logger.log("CAPTURE", &format!("Starting framebuffer capture for handle {:?}", fb));
    }
    
    let img = dump_framebuffer_to_image(card, fb, verbose);
    match img {
        Ok(img) => {
            if let Some(logger) = logger {
                logger.log("CAPTURE", &format!("Successfully captured {}x{} image", img.width(), img.height()));
            }
            
            if let Err(e) = send_dumped_image(socket, &img, verbose) {
                if let Some(logger) = logger {
                    logger.log_error(&format!("Failed to send image to Hyperion: {}", e));
                }
                return Err(e);
            }
            
            if let Some(logger) = logger {
                logger.log("HYPERION", "Image sent successfully");
            }
        }
        Err(e) => {
            let error_msg = format!("Error dumping framebuffer to image: {}", e);
            if let Some(logger) = logger {
                logger.log_error(&error_msg);
            } else {
                println!("{}", error_msg);
            }
        }
    }
    
    // Log current resource state
    if let Some(tracker) = resource_tracker {
        tracker.log_current_state();
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
                .default_value("/dev/dri/card0")
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
        .get_matches();

    let verbose = matches.is_present("verbose");
    let screenshot = matches.is_present("screenshot");
    let diagnostic_mode = matches.is_present("diagnostic");
    let log_file = matches.value_of("log-file").unwrap();
    let monitor_interval: u64 = matches.value_of("monitor-interval")
        .unwrap()
        .parse()
        .expect("Invalid monitor interval");
    
    // Initialize diagnostic logging
    let logger = if diagnostic_mode || verbose {
        match DiagnosticLogger::new(log_file) {
            Ok(logger) => {
                let logger = Arc::new(logger);
                logger.log("INIT", "Diagnostic logging initialized");
                Some(logger)
            }
            Err(e) => {
                eprintln!("Failed to initialize diagnostic logging: {}", e);
                None
            }
        }
    } else {
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

    let adress = matches.value_of("address").unwrap();
    if screenshot {
        if let Some(fb) = find_framebuffer(&card, verbose) {
            let img = dump_framebuffer_to_image(&card, fb, verbose).unwrap();
            save_screenshot(&img).unwrap();
        } else {
            println!("No framebuffer found!");
        }
    } else {
        let mut socket = TcpStream::connect(adress).unwrap();
        register_direct(&mut socket).unwrap();
        read_reply(&mut socket, verbose).unwrap();

        send_color_red(&mut socket, verbose).unwrap();
        thread::sleep(Duration::from_secs(1));

        loop {
            if let Some(fb) = find_framebuffer(&card, verbose) {
                let result = dump_and_send_framebuffer(
                    &mut socket, 
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
                }
                
                thread::sleep(Duration::from_millis(1000/20));
            } else {
                if let Some(ref logger) = logger {
                    logger.log_warning("No framebuffer found, waiting...");
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
