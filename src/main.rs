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
#[allow(mismatched_lifetime_syntaxes)]
pub mod hyperion_reply_generated;
#[allow(mismatched_lifetime_syntaxes)]
pub mod hyperion_request_generated;
pub mod image_decoder;
pub mod dump_image;
pub mod diagnostics;
pub mod system_monitor;
pub mod connection_manager;

pub use hyperion_request_generated::hyperionnet::{Clear, Color, Command, Image, Register};
use hyperion::{read_reply, register_direct, send_image};

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

fn send_dumped_image(socket: &mut TcpStream, img: &RgbImage, verbose: bool) -> StdResult<()> {
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
) -> StdResult<()> {
    let img = dump_framebuffer_to_image(card, fb, verbose);
    if let Ok(img) = img {
        send_dumped_image(socket, &img, verbose)?;
    } else if verbose {
        eprintln!("Error dumping framebuffer to image.");
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

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

fn main() {
    let matches = App::new("DRM VC4 Screen Grabber for Hyperion")
        .version("0.1.2")
        .author("Rudi Horn <dyn-git@rudi-horn.de>")
        .about("Captures a screenshot and sends it to the Hyperion or HyperHDR server.")
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
        .get_matches();

    let verbose = matches.is_present("verbose");
    let screenshot = matches.is_present("screenshot");
    let device_path = matches.value_of("device").unwrap();
    let card = Card::open(device_path);
    let authenticated = card.authenticated().unwrap();

    if verbose {
        let driver = card.get_driver().unwrap();
        println!("Driver (auth={}): {:?}", authenticated, driver);
    }

    unsafe {
        let set_cap = drm_set_client_cap {
            capability: drm_ffi::DRM_CLIENT_CAP_UNIVERSAL_PLANES as u64,
            value: 1,
        };
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
        let mut socket = TcpStream::connect(address).unwrap();
        register_direct(&mut socket).unwrap();
        read_reply(&mut socket, verbose).unwrap();

        if verbose {
            println!("Connected to Hyperion, starting capture loop");
        }

        // Track consecutive errors for connection reliability
        let consecutive_errors = Arc::new(AtomicU32::new(0));
        // Track consecutive "no framebuffer" occurrences
        let mut no_fb_count: u32 = 0;

        loop {
            if let Some(fb) = find_framebuffer(&card, verbose) {
                no_fb_count = 0; // Reset counter on successful find
                match dump_and_send_framebuffer(&mut socket, &card, fb, verbose) {
                    Ok(_) => {
                        consecutive_errors.store(0, Ordering::Relaxed);
                        thread::sleep(Duration::from_millis(33)); // ~30 FPS
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                        eprintln!("HyperHDR disconnected. Reconnecting...");
                        consecutive_errors.store(0, Ordering::Relaxed);
                        thread::sleep(Duration::from_secs(2));

                        match TcpStream::connect(address) {
                            Ok(new_socket) => {
                                socket = new_socket;
                                let _ = register_direct(&mut socket);
                                let _ = read_reply(&mut socket, verbose);
                                eprintln!("Reconnected to HyperHDR");
                            }
                            Err(e) => {
                                eprintln!("Reconnection failed: {}. Will retry...", e);
                            }
                        }
                    }
                    Err(e) => {
                        let errors = consecutive_errors.fetch_add(1, Ordering::Relaxed) + 1;

                        if verbose {
                            eprintln!("Capture error #{}: {}", errors, e);
                        }

                        // Back off on errors
                        let backoff = match errors {
                            1..=2 => 100,
                            3..=5 => 500,
                            _ => 2000,
                        };

                        thread::sleep(Duration::from_millis(backoff));
                    }
                }
            } else {
                no_fb_count += 1;

                if verbose {
                    eprintln!("No framebuffer found (count: {}), waiting...", no_fb_count);
                }

                // Don't send any color - just wait silently
                // The LEDs will maintain their last state or timeout naturally
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
