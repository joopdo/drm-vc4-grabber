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

pub use hyperion_request_generated::hyperionnet::{Clear, Color, Command, Image, Register};

use hyperion::{read_reply, register_direct, send_color_red, send_image};

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
) -> StdResult<()> {
    let img = dump_framebuffer_to_image(card, fb, verbose);
    if let Ok(img) = img {
        send_dumped_image(socket, &img, verbose)?;
    } else {
        println!("Error dumping framebuffer to image.");
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
use std::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::sync::Arc;

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

        // Track consecutive errors and 4K state
        let consecutive_errors = Arc::new(AtomicU32::new(0));
        let mut in_4k_mode = false;
        let mut last_4k_check = std::time::Instant::now();

        loop {
            if let Some(fb) = find_framebuffer(&card, verbose) {
                // Check resolution periodically
                if !in_4k_mode || last_4k_check.elapsed() > Duration::from_secs(5) {
                    match ffi::fb_cmd2(card.as_raw_fd(), fb.into()) {
                        Ok(fbinfo) => {
                            let is_4k = fbinfo.width >= 3840 || fbinfo.height >= 2160;

                            if is_4k && !in_4k_mode {
                                eprintln!("4K content detected ({}x{}), pausing capture", fbinfo.width, fbinfo.height);
                                // Send warmcolor frame to turn off lights
                                let _ = send_color_warm(&mut socket, verbose);
                                in_4k_mode = true;
                            } else if !is_4k && in_4k_mode {
                                eprintln!("HD content detected ({}x{}), resuming capture", fbinfo.width, fbinfo.height);
                                in_4k_mode = false;
                                consecutive_errors.store(0, Ordering::Relaxed);
                            }

                            last_4k_check = std::time::Instant::now();
                        }
                        Err(_) => {}
                    }
                }

                // Skip capture if 4K is playing
                if in_4k_mode {
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }

                // Normal capture for non-4K content
                match dump_and_send_framebuffer(&mut socket, &card, fb, verbose) {
                    Ok(_) => {
                        consecutive_errors.store(0, Ordering::Relaxed);
                        thread::sleep(Duration::from_millis(33)); // 30 FPS
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                        eprintln!("HyperHDR disconnected. Reconnecting...");
                        consecutive_errors.store(0, Ordering::Relaxed);
                        thread::sleep(Duration::from_secs(2));

                        match TcpStream::connect(adress) {
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
                if verbose {
                    eprintln!("No framebuffer found, waiting...");
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
// Replace the send_color_warm function with this:
fn send_color_warm(socket: &mut TcpStream, verbose: bool) -> StdResult<()> {
    // Based on the pattern from send_color_red in the existing code
    use hyperion::send_image;
    use image::RgbImage;

    // Create a 1x1 warm yellow/white image
    let mut img = RgbImage::new(1, 1);
    img.put_pixel(0, 0, image::Rgb([255, 200, 100]));

    send_image(socket, &img, verbose)
}
