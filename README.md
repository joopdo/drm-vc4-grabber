# Hyperion & HyperHDR DRM VC4 screen grabber

Captures the Raspberry Pi's framebuffer directly from the DRM (Direct Rendering
Manager) subsystem and streams it to [Hyperion] / [HyperHDR] for ambient LED
backlighting. It does not rely on a Kodi add-on, an X server, or a separate
grabber binary: it reads the framebuffer that Kodi (or any KMS-native
application) is already rendering, decodes it, and ships the image over TCP.

[Hyperion]: https://github.com/hyperion-project/hyperion.ng
[HyperHDR]: https://github.com/awawa-dev/HyperHDR

## Supported platforms

- **Raspberry Pi 5** (LibreELEC, driver `vc4` exposed as `card1`)
- **Raspberry Pi 4** (LibreELEC / Raspberry Pi OS, driver `vc4` at `card0`)
- Earlier Pi models that expose the VC4 KMS driver

Pi 5 and Pi 4 have the display on different card nodes. Use the `-d` flag to
override the default if needed (see [Usage](#usage)).

## Supported framebuffer formats

The grabber dispatches on the framebuffer's DRM FourCC and modifier:

| FourCC        | Modifier                 | Common use                        |
|---------------|--------------------------|-----------------------------------|
| `XRGB8888`    | Linear / VC4 T-tiled     | Kodi UI, SDR video                |
| `ARGB8888`    | Linear / VC4 T-tiled     | Kodi UI with alpha                |
| `XRGB2101010` | Linear / VC4 T-tiled     | 10-bit HDR content                |
| `YUV420`      | Linear                   | SD/HD video (8-bit)               |
| `NV12`        | Broadcom SAND128         | Hardware-decoded HD video         |
| `P030`        | Broadcom SAND128         | Hardware-decoded 4K HDR video     |
| `RGB565`      | Linear                   | Legacy low-bpp surfaces           |

Unknown formats are skipped with a log message, not a crash, so a transient
format switch (common during HDR metadata transitions) will not take the
service down.

## Usage

1. Download the latest release from the [Releases] page and extract it:
   ```
   tar xf drm-vc4-grabber-vX.Y.Z-aarch64-linux.tar.xz
   ```
2. Run it directly, pointing at your Hyperion server:
   ```
   ./drm-vc4-grabber-vX.Y.Z-aarch64-linux/drm-vc4-grabber
   ```
3. For a permanent setup, copy the supplied `drm-capture.service` unit to your
   systemd config path. On LibreELEC:
   ```
   cp drm-capture.service /storage/.config/system.d/
   systemctl enable --now drm-capture.service
   ```

[Releases]: https://github.com/rudihorn/drm-vc4-grabber/releases

### Command-line flags

| Flag | Default | Description |
|------|---------|-------------|
| `-d`, `--device` | `/dev/dri/card1` | DRM card node. Use `card0` for Pi 4 and earlier. |
| `-a`, `--address` | `127.0.0.1:19400` | Hyperion / HyperHDR TCP endpoint. |
| `--poll-ms` | `66` | How often to look up which framebuffer the video plane shows (one `GETPLANE`). |
| `--max-fps` | `15` | Upper bound on real captures per second. |
| `--screenshot` | off | Capture a single frame to `screenshot.png` and exit (also prints the plane and on-screen geometry it found). |
| `--profile` | off | Print timing, poll/skip and plane-lookup statistics every 2 s. |
| `--unthrottled` | off | No frame pacing (profiling only, not for production). |
| `-v`, `--verbose` | off | Log DRM, framebuffer, and send details. |

## Building from source

The easiest cross-compilation path uses `cargo-zigbuild`, which bundles its own
toolchain via `pip install ziglang`. No system cross-compiler or sudo required.

```bash
# one-time setup
rustup target add aarch64-unknown-linux-musl
cargo install cargo-zigbuild
pip install --user ziglang

# build
cargo zigbuild --release --target aarch64-unknown-linux-musl
```

The resulting binary is at
`target/aarch64-unknown-linux-musl/release/drm-vc4-grabber`.

Native builds on x86_64 (for compile-checks only, not runnable) work with
`cargo build --release`.

## How it works

At startup the grabber opens the DRM device and enables the `UNIVERSAL_PLANES`
and `ATOMIC` client capabilities. `ATOMIC` is only used to *read* the plane
properties `CRTC_X/Y/W/H` and `SRC_X/Y/W/H` (where the picture is on screen);
the grabber never commits anything, Kodi stays the only DRM master.

1. **Find the picture.** Every 2 s all planes are scanned (atomic state via
   `GETPLANE`; the legacy CRTC framebuffer can be stale). The plane carrying a
   video format (`P030`, `NV12`, `YUV420`) is preferred, otherwise the GUI plane.
   Its on-screen rectangle, source crop and the CRTC mode size are read then.
2. **Cheap lookups.** Between scans, one `GETPLANE` per tick (`--poll-ms`) gives
   the framebuffer currently shown. A real capture only happens when it changed
   (or every 500 ms), at most `--max-fps` per second. On a Pi 5 each DRM query
   waits behind Kodi's atomic commit (measured ~17 ms), so fewer queries means
   less contention with video output.
3. **Capture.** `GETFB2`, PRIME export (`O_CLOEXEC`), `poll` for readiness (2 ms
   budget, otherwise the LED frame is dropped), `DMA_BUF_IOCTL_SYNC` START,
   `mmap`, sparse sampling, SYNC END, `munmap`, close everything. No DRM
   resource is kept between captures.
4. **Place it on screen.** The sample is drawn at its real position on a fixed
   160x90 canvas; everything else is black. Portrait videos, pillar- and
   letterboxing therefore give real black bars, and padding rows (e.g. a
   1920x1088 decoder buffer showing 1080 lines) are cropped.
5. **Send.** Flatbuffer image to Hyperion/HyperHDR. While nothing changes, the
   cached image is re-sent every 500 ms without touching DRM. Slow or failing
   captures back off automatically.

The output image is always 160x90, so the server never sees a size change when
the source resolution changes (live TV, resolution switches).

### Black bars and the server's black-border detection

Because the bars are now real black pixels, HyperHDR/Hyperion's *blackbar
detection* would crop them away again and stretch the picture over all LEDs.
If you want the LEDs next to the bars to stay dark (portrait clips, 21:9
films), switch black-border detection off in the server.

## Compatibility with ambient-light servers

The grabber uses Hyperion's native flatbuffer protocol on TCP port 19400. Both
**Hyperion.NG** and **HyperHDR** speak this protocol, so either works as the
receiver without any configuration change in the grabber.

## License

MIT. See [LICENSE](LICENSE).

## Donations

Not required and not solicited, but if you would like to support Rudi Horn
(the original author):

| PayPal | Bitcoin |
|--------|---------|
| [![paypal](https://www.paypalobjects.com/en_US/i/btn/btn_donateCC_LG.gif)](https://www.paypal.me/rudihppal) | [bitcoin:bc1qjantllys0pg3zvsr97krxzz9dxzlmxmgy5qk4v](bitcoin:bc1qjantllys0pg3zvsr97krxzz9dxzlmxmgy5qk4v) |
