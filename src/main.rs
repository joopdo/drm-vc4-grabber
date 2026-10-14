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

pub mod ffi;
pub mod hyperion;
#[allow(mismatched_lifetime_syntaxes)]
pub mod hyperion_reply_generated;
#[allow(mismatched_lifetime_syntaxes)]
pub mod hyperion_request_generated;
pub mod image_decoder;
pub mod dump_image;

use hyperion::{drain_replies, read_reply, register_direct, send_image};

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
    pub fn open(path: &str) -> std::io::Result<Self> {
        let mut options = OpenOptions::new();
        options.read(true);
        options.write(false);
        Ok(Card(options.open(path)?))
    }
}

fn save_screenshot(img: &RgbImage) -> Result<(), ImageError> {
    img.save("screenshot.png")
}

/// Per-frame timing breakdown, emitted when --profile is on.
#[derive(Default, Clone, Copy)]
pub struct FrameStats {
    pub find_fb_us: u64,
    pub decode_us: u64,
    pub send_us: u64,
    pub img_w: u32,
    pub img_h: u32,
}

/// Running aggregator so we can print rolling averages rather than spamming
/// one line per frame.
struct ProfileAggregator {
    frames: u32,
    find_fb_sum: u64,
    decode_sum: u64,
    send_sum: u64,
    decode_max: u64,
    last_img_size: (u32, u32),
    last_print: std::time::Instant,
    format_hint: Option<String>,
}

impl ProfileAggregator {
    fn new() -> Self {
        Self {
            frames: 0,
            find_fb_sum: 0,
            decode_sum: 0,
            send_sum: 0,
            decode_max: 0,
            last_img_size: (0, 0),
            last_print: std::time::Instant::now(),
            format_hint: None,
        }
    }

    fn record(&mut self, s: FrameStats) {
        self.frames += 1;
        self.find_fb_sum += s.find_fb_us;
        self.decode_sum += s.decode_us;
        self.send_sum += s.send_us;
        if s.decode_us > self.decode_max {
            self.decode_max = s.decode_us;
        }
        self.last_img_size = (s.img_w, s.img_h);
    }

    fn set_format(&mut self, fmt: String) {
        self.format_hint = Some(fmt);
    }

    /// Flush an averaged report if enough time has passed. Returns whether
    /// it printed so the caller knows to reset counters.
    fn maybe_flush(&mut self) {
        let elapsed = self.last_print.elapsed();
        if elapsed < std::time::Duration::from_secs(2) {
            return;
        }
        if self.frames == 0 {
            self.last_print = std::time::Instant::now();
            return;
        }
        let n = self.frames as u64;
        let elapsed_s = elapsed.as_secs_f64();
        let actual_fps = (self.frames as f64) / elapsed_s;
        let total_avg_us = (self.find_fb_sum + self.decode_sum + self.send_sum) / n;
        let headroom_fps = if total_avg_us > 0 {
            1_000_000.0 / total_avg_us as f64
        } else {
            f64::INFINITY
        };
        let fmt = self.format_hint.as_deref().unwrap_or("?");
        println!(
            "[profile] fmt={} out={}x{} | actual_fps={:.1} headroom_fps={:.1} | find_fb={}us decode_avg={}us decode_max={}us send={}us | total_avg={}us frames={}",
            fmt,
            self.last_img_size.0,
            self.last_img_size.1,
            actual_fps,
            headroom_fps,
            self.find_fb_sum / n,
            self.decode_sum / n,
            self.decode_max,
            self.send_sum / n,
            total_avg_us,
            self.frames,
        );
        self.frames = 0;
        self.find_fb_sum = 0;
        self.decode_sum = 0;
        self.send_sum = 0;
        self.decode_max = 0;
        self.last_print = std::time::Instant::now();
    }
}

/// Caches per-card DRM topology so we don't re-enumerate CRTCs and planes
/// every frame (those ioctls get expensive during active video playback
/// because they contend with Kodi's continuous framebuffer updates).
///
/// The plane list only changes on hotplug, so it is read once. The plane that
/// carries the picture (the video plane during playback, else the GUI plane)
/// is identified by a scan every RESCAN; in between, one GETPLANE per poll.
/// Where the sampled plane's picture really is on screen, read from the plane's
/// atomic properties at each RESCAN (never per frame).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    screen: (u32, u32),          // CRTC mode size
    dst: (i32, i32, u32, u32),   // CRTC_X, CRTC_Y, CRTC_W, CRTC_H
    src: (u32, u32, u32, u32),   // SRC_X/Y/W/H in fb pixels (16.16 >> 16)
    fb: (u32, u32),              // framebuffer size
}

/// Every image sent to HyperHDR has this size: HyperHDR never sees a size
/// change (resolution switch, live TV), and black bars are real black pixels.
const CANVAS: (u32, u32) = (160, 90);

/// Put the sampled frame where it is on screen, black elsewhere. `img` samples
/// the whole framebuffer; only the SRC crop is visible, at the DST rectangle.
fn compose(img: &RgbImage, g: Option<Geometry>) -> RgbImage {
    let (cw, ch) = CANVAS;
    let mut out = RgbImage::new(cw, ch);
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 { return out; }
    let (mut sx, mut sy, mut sw, mut sh) = (0f32, 0f32, iw as f32, ih as f32);
    let (mut dx, mut dy, mut dw, mut dh) = (0f32, 0f32, cw as f32, ch as f32);
    if let Some(g) = g {
        if g.screen.0 > 0 && g.screen.1 > 0 && g.dst.2 > 0 && g.dst.3 > 0 && g.fb.0 > 0 && g.fb.1 > 0 {
            let (fx, fy) = (iw as f32 / g.fb.0 as f32, ih as f32 / g.fb.1 as f32);
            if g.src.2 > 0 && g.src.3 > 0 {
                sx = g.src.0 as f32 * fx; sy = g.src.1 as f32 * fy;
                sw = g.src.2 as f32 * fx; sh = g.src.3 as f32 * fy;
            }
            let (kx, ky) = (cw as f32 / g.screen.0 as f32, ch as f32 / g.screen.1 as f32);
            dx = g.dst.0 as f32 * kx; dy = g.dst.1 as f32 * ky;
            dw = g.dst.2 as f32 * kx; dh = g.dst.3 as f32 * ky;
        }
    }
    let x0 = dx.max(0.0).floor() as u32;
    let y0 = dy.max(0.0).floor() as u32;
    let x1 = ((dx + dw).min(cw as f32)).ceil() as u32;
    let y1 = ((dy + dh).min(ch as f32)).ceil() as u32;
    // only sample rows/columns that lie completely inside the visible crop
    // (a sample row that is half picture, half padding is left out)
    let (umin, vmin) = (sx.ceil().max(0.0), sy.ceil().max(0.0));
    let umax = ((sx + sw).floor().min(iw as f32) - 1.0).max(umin);
    let vmax = ((sy + sh).floor().min(ih as f32) - 1.0).max(vmin);
    for y in y0..y1.min(ch) {
        let v = ((y as f32 + 0.5 - dy) / dh * sh + sy).floor().clamp(vmin, vmax);
        for x in x0..x1.min(cw) {
            let u = ((x as f32 + 0.5 - dx) / dw * sw + sx).floor().clamp(umin, umax);
            out.put_pixel(x, y, *img.get_pixel(u as u32, v as u32));
        }
    }
    out
}

struct FramebufferFinder {
    planes: Vec<drm::control::plane::Handle>,
    /// Plane currently showing the picture we sample (video plane if any).
    cached: Option<drm::control::plane::Handle>,
    last_scan: Instant,
    // stats for --profile: fast-path GETPLANE latency and distinct FB ids
    lookups: u64,
    lookup_us_sum: u64,
    lookup_us_max: u64,
    distinct: std::collections::HashSet<u32>,
    geometry: Option<Geometry>,
    /// a geometry result (also "unknown") has been logged at least once
    geometry_logged: bool,
}

/// Video formats Kodi puts on a plane (direct-to-plane rendering).
const FOURCC_P030: u32 = 808661072;
const FOURCC_NV12: u32 = 0x3231_564E;
const FOURCC_YUV420: u32 = 0x3231_5559;
/// Re-identify the video plane this often (video start/stop, GUI vs video).
const RESCAN: Duration = Duration::from_secs(2);

impl FramebufferFinder {
    fn new(card: &Card) -> Option<Self> {
        let plane_handles = card.plane_handles().ok()?;
        Some(FramebufferFinder {
            planes: plane_handles.planes().to_vec(),
            cached: None,
            last_scan: Instant::now(),
            lookups: 0,
            lookup_us_sum: 0,
            lookup_us_max: 0,
            distinct: std::collections::HashSet::new(),
            geometry: None,
            geometry_logged: false,
        })
    }

    fn plane_id(&self) -> Option<u32> {
        self.cached.map(u32::from)
    }

    /// Pixel format of a framebuffer. GETFB2 creates GEM handles in our file;
    /// close each unique one immediately.
    fn fb_format(card: &Card, fb: Handle) -> Option<u32> {
        Self::fb_info(card, fb).map(|i| i.0)
    }

    /// (pixel format, width, height). GETFB2 creates GEM handles; close them.
    fn fb_info(card: &Card, fb: Handle) -> Option<(u32, u32, u32)> {
        let info = ffi::fb_cmd2(card.as_raw_fd(), fb.into()).ok()?;
        let mut closed: Vec<u32> = Vec::with_capacity(4);
        for h in info.handles.iter().copied().filter(|h| *h != 0) {
            if !closed.contains(&h) {
                let _ = ffi::gem_close(card.as_raw_fd(), h);
                closed.push(h);
            }
        }
        Some((info.pixel_format, info.width, info.height))
    }

    /// Read the plane's on-screen rectangle and source crop (atomic plane
    /// properties) plus the CRTC mode size. Only called from scan().
    fn read_geometry(card: &Card, plane: drm::control::plane::Handle, fb: Handle) -> Option<Geometry> {
        let pinfo = card.get_plane(plane).ok()?;
        let crtc = pinfo.crtc()?;
        let mode = card.get_crtc(crtc).ok()?.mode()?;
        let (mw, mh) = mode.size();
        let props = card.get_properties(plane).ok()?;
        let (ids, vals) = props.as_props_and_values();
        let mut v = std::collections::HashMap::new();
        for (id, val) in ids.iter().zip(vals.iter()) {
            if let Ok(info) = card.get_property(*id) {
                if let Ok(name) = info.name().to_str() {
                    v.insert(name.to_string(), *val);
                }
            }
        }
        let g = |n: &str| v.get(n).copied();
        let (_, fw, fh) = Self::fb_info(card, fb)?;
        Some(Geometry {
            screen: (mw as u32, mh as u32),
            dst: (g("CRTC_X")? as i64 as i32, g("CRTC_Y")? as i64 as i32, g("CRTC_W")? as u32, g("CRTC_H")? as u32),
            src: ((g("SRC_X")? >> 16) as u32, (g("SRC_Y")? >> 16) as u32, (g("SRC_W")? >> 16) as u32, (g("SRC_H")? >> 16) as u32),
            fb: (fw, fh),
        })
    }

    fn geometry(&self) -> Option<Geometry> {
        self.geometry
    }

    /// Walk all planes (atomic state via GETPLANE; the legacy CRTC fb can be
    /// stale), prefer the plane carrying a video format, else the first
    /// active plane. Runs at most every RESCAN or when the cached plane loses
    /// its framebuffer.
    fn scan(&mut self, card: &Card, verbose: bool) -> Option<Handle> {
        self.last_scan = Instant::now();
        let mut first = None;
        let mut video = None;
        for plane in &self.planes {
            let info = match card.get_plane(*plane) {
                Ok(i) => i,
                Err(e) => {
                    if verbose { eprintln!("get_plane({:?}) failed: {}", plane, e); }
                    continue;
                }
            };
            if info.crtc().is_none() { continue; }
            if let Some(fb) = info.framebuffer() {
                if first.is_none() { first = Some((*plane, fb)); }
                if video.is_none() {
                    if let Some(f) = Self::fb_format(card, fb) {
                        if f == FOURCC_P030 || f == FOURCC_NV12 || f == FOURCC_YUV420 {
                            video = Some((*plane, fb));
                        }
                    }
                }
            }
        }
        let pick = video.or(first);
        let new_plane = pick.map(|(p, _)| p);
        let geo = pick.and_then(|(p, fb)| Self::read_geometry(card, p, fb));
        if geo != self.geometry || !self.geometry_logged {
            self.geometry_logged = true;
            match geo {
                Some(g) => eprintln!("[grabber] picture at {},{} {}x{} on {}x{} screen (visible fb part {},{} {}x{} of {}x{})",
                                     g.dst.0, g.dst.1, g.dst.2, g.dst.3, g.screen.0, g.screen.1,
                                     g.src.0, g.src.1, g.src.2, g.src.3, g.fb.0, g.fb.1),
                None => eprintln!("[grabber] geometry unknown: frame fills the canvas"),
            }
            self.geometry = geo;
        }
        if new_plane != self.cached {
            eprintln!("[grabber] sampling plane {:?} ({})", new_plane.map(u32::from),
                      if video.is_some() { "video" } else { "gui" });
        }
        self.cached = new_plane;
        pick.map(|(_, fb)| fb)
    }

    /// Framebuffer currently shown on the sampled plane: one GETPLANE on the
    /// fast path, a scan every RESCAN or on a miss.
    fn find(&mut self, card: &Card, verbose: bool) -> Option<Handle> {
        let plane = match self.cached {
            Some(p) if self.last_scan.elapsed() < RESCAN => p,
            _ => return self.scan(card, verbose),
        };
        let t = Instant::now();
        let res = card.get_plane(plane);
        let us = t.elapsed().as_micros() as u64;
        self.lookups += 1;
        self.lookup_us_sum += us;
        if us > self.lookup_us_max { self.lookup_us_max = us; }
        if let Ok(info) = res {
            if info.crtc().is_some() {
                if let Some(fb) = info.framebuffer() {
                    self.distinct.insert(fb.into());
                    return Some(fb);
                }
            }
        }
        self.scan(card, verbose)
    }

    /// (lookups, avg us, max us, distinct fb ids) since the last call.
    fn take_stats(&mut self) -> (u64, u64, u64, usize) {
        let r = (self.lookups,
                 if self.lookups > 0 { self.lookup_us_sum / self.lookups } else { 0 },
                 self.lookup_us_max, self.distinct.len());
        self.lookups = 0; self.lookup_us_sum = 0; self.lookup_us_max = 0; self.distinct.clear();
        r
    }
}

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Target capture rate. 30 FPS is the practical upper bound for Hyperion — it
/// smooths LEDs internally and higher rates just burn CPU on the Pi.
/// A full grab is forced this often even if the plane still shows the same
/// framebuffer id: ids are reused after RMFB and KMS allows front-buffer
/// updates, so "same id" is a best-effort hint, never proof of same pixels.
/// 500 ms also keeps HyperHDR fed (image duration is 1000 ms, see send_image).
const REGRAB: Duration = Duration::from_millis(500);

/// While nothing new is grabbed, resend the cached small RGB image this often
/// so HyperHDR keeps the input (it expires 1000 ms after the last image). This
/// touches no DRM resource at all.
const HEARTBEAT: Duration = Duration::from_millis(500);

/// A grab slower than this counts as "slow" for backpressure.
const SLOW_GRAB: Duration = Duration::from_millis(15);

/// Integer CLI option within [min, max], else the default (with a warning).
fn int_arg(matches: &clap::ArgMatches, name: &str, min: u64, max: u64, default: u64) -> u64 {
    match matches.value_of(name).and_then(|v| v.parse::<u64>().ok()) {
        Some(v) if v >= min && v <= max => v,
        _ => {
            eprintln!("--{} must be {}-{}, using {}", name, min, max, default);
            default
        }
    }
}

fn main() {
    let matches = App::new("DRM VC4 Screen Grabber for Hyperion")
        .version("0.2.0")
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
        .arg(
            Arg::with_name("profile")
                .long("profile")
                .help("Emit periodic per-frame timing stats (for CPU investigation)."),
        )
        .arg(
            Arg::with_name("poll-ms")
                .long("poll-ms")
                .default_value("66")
                .takes_value(true)
                .help("How often (2-100 ms) to check which framebuffer is on screen. Cheap: one GETPLANE, no buffer access. Sets the LED latency."),
        )
        .arg(
            Arg::with_name("max-fps")
                .long("max-fps")
                .default_value("15")
                .takes_value(true)
                .help("Upper bound (1-60) on full grabs per second. A grab only happens when a new framebuffer is shown (or every 500 ms). Conservative default while stability is tested; raise for lower LED lag."),
        )
        .arg(
            Arg::with_name("unthrottled")
                .long("unthrottled")
                .help("Run the capture loop without the 33ms frame cap. Combine with --profile to see the maximum pipeline throughput; useful for CPU investigation. Do not use in production."),
        )
        .get_matches();

    let verbose = matches.is_present("verbose");
    let profile = matches.is_present("profile");
    let unthrottled = matches.is_present("unthrottled");
    let screenshot = matches.is_present("screenshot");
    let device_path = matches.value_of("device").unwrap();
    let poll = Duration::from_millis(int_arg(&matches, "poll-ms", 2, 100, 66));
    let base_grab = Duration::from_micros(1_000_000 / int_arg(&matches, "max-fps", 1, 60, 15));
    let card = match Card::open(device_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open DRM device '{}': {}", device_path, e);
            std::process::exit(1);
        }
    };
    let authenticated = card.authenticated().unwrap_or(false);

    if verbose {
        if let Ok(driver) = card.get_driver() {
            println!("Driver (auth={}): {:?}", authenticated, driver);
        }
    }

    // Enable universal planes so we can see overlay/cursor planes too. Without
    // this we only see the primary plane which is often not where video lands.
    unsafe {
        let set_cap = drm_set_client_cap {
            capability: drm_ffi::DRM_CLIENT_CAP_UNIVERSAL_PLANES as u64,
            value: 1,
        };
        if let Err(e) = drm_ffi::ioctl::set_cap(card.as_raw_fd(), &set_cap) {
            eprintln!("Warning: could not enable UNIVERSAL_PLANES: {}", e);
        }
        // The plane properties CRTC_X/Y/W/H and SRC_* (where the picture is on
        // screen) are only exposed to clients that announce atomic support.
        // Reading only: we never commit, Kodi stays the only DRM master.
        let set_atomic = drm_set_client_cap {
            capability: drm_ffi::DRM_CLIENT_CAP_ATOMIC as u64,
            value: 1,
        };
        if let Err(e) = drm_ffi::ioctl::set_cap(card.as_raw_fd(), &set_atomic) {
            eprintln!("Warning: could not enable ATOMIC (plane position unknown, frame fills the canvas): {}", e);
        }
    }

    let address = matches.value_of("address").unwrap();
    
    if screenshot {
        let mut finder = match FramebufferFinder::new(&card) {
            Some(f) => f,
            None => {
                eprintln!("Failed to enumerate DRM resources");
                std::process::exit(1);
            }
        };
        if let Some(fb) = finder.find(&card, verbose) {
            let frame = dump_framebuffer_to_image(&card, fb, verbose).unwrap();
            save_screenshot(&frame.image).unwrap();
        } else {
            println!("No framebuffer found!");
        }
    } else {
        // Establish the Hyperion/HyperHDR connection and register just once.
        // Re-registering on every frame (as the original code did) doubles
        // round-trips per frame and spams the server's priority registry.
        let mut socket = TcpStream::connect(address).unwrap();
        register_direct(&mut socket).unwrap();
        read_reply(&mut socket, verbose).unwrap();

        if verbose {
            println!("Connected to Hyperion, starting capture loop");
        }
        if profile {
            println!("[profile] profiling enabled, reports every 2s");
        }
        if unthrottled {
            println!("[profile] UNTHROTTLED: frame pacing disabled, loop runs as fast as possible");
        }

        let consecutive_errors = Arc::new(AtomicU32::new(0));
        let mut no_fb_count: u32 = 0;
        let mut prof = ProfileAggregator::new();
        let mut last_drain = Instant::now();
        let mut finder = match FramebufferFinder::new(&card) {
            Some(f) => f,
            None => {
                eprintln!("Failed to enumerate DRM resources at startup");
                std::process::exit(1);
            }
        };

        // (plane, framebuffer) of the last successful grab; None = unknown, grab
        // at the next poll. Reset after no-frame, errors and reconnects.
        let mut last_key: Option<(Option<u32>, u32)> = None;
        let mut last_grab = Instant::now().checked_sub(REGRAB).unwrap_or_else(Instant::now);
        let mut last_format: &'static str = "";
        // Backpressure: two slow grabs in a row halve the grab rate for 2 s; repeated
        // errors drop to 2 grabs/s. Recovers automatically, never "catches up".
        let mut min_grab = base_grab;
        let mut slow_streak = 0u32;
        let mut backoff_until = Instant::now();
        let mut cached: Option<RgbImage> = None;
        let mut last_send = Instant::now();
        let (mut n_polls, mut n_skips, mut n_not_ready) = (0u64, 0u64, 0u64);
        let mut counters_at = Instant::now();

        loop {
            let frame_start = Instant::now();
            if min_grab != base_grab && Instant::now() >= backoff_until && consecutive_errors.load(Ordering::Relaxed) == 0 {
                min_grab = base_grab;
            }
            if !unthrottled && last_send.elapsed() >= HEARTBEAT {
                if let Some(img) = &cached {
                    if send_image(&mut socket, img, verbose).is_ok() {
                        last_send = Instant::now();
                    }
                }
            }
            if profile && counters_at.elapsed() >= Duration::from_secs(2) {
                let (lk, lk_avg, lk_max, nfb) = finder.take_stats();
                println!("[profile] polls={} skipped={} not_ready={} getplane n={} avg={}us max={}us distinct_fb={} plane={:?}",
                         n_polls, n_skips, n_not_ready, lk, lk_avg, lk_max, nfb, finder.plane_id());
                n_polls = 0; n_skips = 0; n_not_ready = 0;
                counters_at = Instant::now();
            }

            // Every ~1s, drain any replies HyperHDR has sent us so its acks
            // don't slowly fill our TCP receive buffer. Non-blocking, cheap.
            if last_drain.elapsed() >= Duration::from_secs(1) {
                if let Err(e) = drain_replies(&mut socket) {
                    if verbose {
                        eprintln!("drain_replies warning: {}", e);
                    }
                }
                last_drain = Instant::now();
            }

            // --- stage: find the active framebuffer handle
            let t0 = Instant::now();
            let fb_opt = finder.find(&card, verbose);
            let find_fb_us = t0.elapsed().as_micros() as u64;

            if let Some(fb) = fb_opt {
                no_fb_count = 0;
                n_polls += 1;
                let key = (finder.plane_id(), u32::from(fb));
                let since = last_grab.elapsed();
                if !unthrottled && ((last_key == Some(key) && since < REGRAB) || since < min_grab) {
                    n_skips += 1;
                    thread::sleep(poll.saturating_sub(frame_start.elapsed()));
                    continue;
                }

                // --- stage: decode framebuffer to RGB image
                let t1 = Instant::now();
                let decode_result = dump_framebuffer_to_image(&card, fb, verbose);
                let decode_us = t1.elapsed().as_micros() as u64;

                match decode_result {
                    Ok(frame) => {
                        let format_label = frame.format_label;
                        let img = compose(&frame.image, finder.geometry());
                        if format_label != last_format {
                            // Log every transition (rare), so tests can see which plane/format was read.
                            eprintln!("[grabber] plane={:?} fb={} format={} -> {}x{} sample", key.0, key.1, format_label, img.width(), img.height());
                            last_format = format_label;
                        }

                        // --- stage: encode + send to Hyperion
                        let t2 = Instant::now();
                        let send_result = send_image(&mut socket, &img, verbose);
                        let send_us = t2.elapsed().as_micros() as u64;

                        match send_result {
                            Ok(_) => {
                                consecutive_errors.store(0, Ordering::Relaxed);
                                last_key = Some(key);
                                last_grab = Instant::now();
                                last_send = last_grab;
                                if Duration::from_micros(decode_us) > SLOW_GRAB {
                                    slow_streak += 1;
                                    if slow_streak >= 2 {
                                        min_grab = base_grab * 2;
                                        backoff_until = Instant::now() + Duration::from_secs(2);
                                        if profile { println!("[profile] backoff: slow grabs ({}us)", decode_us); }
                                    }
                                } else {
                                    slow_streak = 0;
                                }
                                if profile {
                                    prof.set_format(format_label.to_string());
                                    prof.record(FrameStats {
                                        find_fb_us,
                                        decode_us,
                                        send_us,
                                        img_w: img.width(),
                                        img_h: img.height(),
                                    });
                                    prof.maybe_flush();
                                }
                                cached = Some(img);
                                if !unthrottled {
                                    thread::sleep(poll.saturating_sub(frame_start.elapsed()));
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                                eprintln!("Hyperion disconnected. Reconnecting...");
                                last_key = None;
                                consecutive_errors.store(0, Ordering::Relaxed);
                                thread::sleep(Duration::from_secs(2));

                                match TcpStream::connect(address) {
                                    Ok(new_socket) => {
                                        socket = new_socket;
                                        if let Err(e) = register_direct(&mut socket) {
                                            eprintln!("Re-register failed: {}", e);
                                            continue;
                                        }
                                        if let Err(e) = read_reply(&mut socket, verbose) {
                                            eprintln!("Re-register read_reply failed: {}", e);
                                            continue;
                                        }
                                        eprintln!("Reconnected to Hyperion");
                                    }
                                    Err(e) => {
                                        eprintln!("Reconnection failed: {}. Will retry...", e);
                                    }
                                }
                            }
                            Err(e) => {
                                last_key = None;
                                let errors = consecutive_errors.fetch_add(1, Ordering::Relaxed) + 1;
                                if verbose {
                                    eprintln!("Send error #{}: {}", errors, e);
                                }
                                let backoff_ms = match errors {
                                    1..=2 => 100,
                                    3..=5 => 500,
                                    _ => 2000,
                                };
                                thread::sleep(Duration::from_millis(backoff_ms));
                            }
                        }
                    }
                    Err(drm::SystemError::Unknown { errno }) if errno == dump_image::NOT_READY => {
                        // Producer still writing: drop this LED frame, never wait on Kodi.
                        n_not_ready += 1;
                        thread::sleep(poll.saturating_sub(frame_start.elapsed()));
                    }
                    Err(e) => {
                        last_key = None;
                        min_grab = Duration::from_millis(500);
                        backoff_until = Instant::now() + Duration::from_secs(2);
                        // Decode failed (e.g. transient unknown format during
                        // HDR metadata switch). Count it and back off, but
                        // don't bail out — the next frame may succeed.
                        let errors = consecutive_errors.fetch_add(1, Ordering::Relaxed) + 1;
                        if verbose || profile {
                            eprintln!("Decode error #{}: {:?}", errors, e);
                        }
                        let backoff_ms = match errors {
                            1..=2 => 100,
                            3..=5 => 500,
                            _ => 2000,
                        };
                        thread::sleep(Duration::from_millis(backoff_ms));
                    }
                }
            } else {
                last_key = None;
                no_fb_count += 1;

                if verbose {
                    eprintln!("No framebuffer found (count: {}), waiting...", no_fb_count);
                }

                // No framebuffer usually means the compositor is transitioning.
                // Sleeping silently preserves the last LED state rather than
                // forcing a colour that would jank the wall.
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

#[cfg(test)]
mod canvas_tests {
    use super::*;
    use image::Rgb;
    fn solid(w: u32, h: u32, c: [u8; 3]) -> RgbImage { RgbImage::from_pixel(w, h, Rgb(c)) }
    fn geo(screen: (u32, u32), dst: (i32, i32, u32, u32), src: (u32, u32, u32, u32), fb: (u32, u32)) -> Option<Geometry> {
        Some(Geometry { screen, dst, src, fb })
    }
    #[test]
    fn full_screen_fills_canvas() {
        let out = compose(&solid(120, 67, [200, 10, 10]), geo((3840, 2160), (0, 0, 3840, 2160), (0, 0, 1920, 1080), (1920, 1080)));
        assert_eq!(out.dimensions(), CANVAS);
        assert!(out.pixels().all(|p| p.0 == [200, 10, 10]));
    }
    #[test]
    fn portrait_clip_has_black_sides() {
        // 1080x1920 portrait clip shown centred on a 3840x2160 screen: 1215 px wide
        let out = compose(&solid(68, 120, [0, 200, 0]), geo((3840, 2160), (1312, 0, 1215, 2160), (0, 0, 1080, 1920), (1080, 1920)));
        assert_eq!(out.get_pixel(5, 45).0, [0, 0, 0], "left bar black");
        assert_eq!(out.get_pixel(154, 45).0, [0, 0, 0], "right bar black");
        assert_eq!(out.get_pixel(80, 45).0, [0, 200, 0], "picture in the middle");
        assert_eq!(out.get_pixel(80, 0).0, [0, 200, 0], "full height");
    }
    #[test]
    fn letterbox_film_has_black_top_and_bottom() {
        // 2.39:1 picture letterboxed on 16:9: 3840x1606 at y=277
        let out = compose(&solid(160, 67, [9, 9, 250]), geo((3840, 2160), (0, 277, 3840, 1606), (0, 0, 1920, 803), (1920, 803)));
        assert_eq!(out.get_pixel(80, 2).0, [0, 0, 0]);
        assert_eq!(out.get_pixel(80, 87).0, [0, 0, 0]);
        assert_eq!(out.get_pixel(80, 45).0, [9, 9, 250]);
    }
    #[test]
    fn padding_rows_of_1088_buffer_are_cropped() {
        // fb 1920x1088, only 1080 visible: bottom 8 rows (red) must not reach the canvas
        let mut img = solid(120, 68, [50, 50, 50]);
        for x in 0..120 { img.put_pixel(x, 67, Rgb([255, 0, 0])); }
        let out = compose(&img, geo((3840, 2160), (0, 0, 3840, 2160), (0, 0, 1920, 1080), (1920, 1088)));
        assert!(out.pixels().all(|p| p.0 != [255, 0, 0]), "padding row leaked into the picture");
    }
    #[test]
    fn unknown_geometry_still_fixed_size() {
        let out = compose(&solid(53, 30, [1, 2, 3]), None);
        assert_eq!(out.dimensions(), CANVAS);
        assert!(out.pixels().all(|p| p.0 == [1, 2, 3]));
    }
    #[test]
    fn offscreen_or_zero_rect_does_not_panic() {
        let _ = compose(&solid(10, 10, [1, 1, 1]), geo((3840, 2160), (-500, -500, 100, 100), (0, 0, 10, 10), (10, 10)));
        let _ = compose(&solid(10, 10, [1, 1, 1]), geo((3840, 2160), (0, 0, 0, 0), (0, 0, 0, 0), (0, 0)));
        let _ = compose(&solid(0, 0, [1, 1, 1]), None);
    }
}
