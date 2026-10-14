use drm_ffi::result::SystemError;
use drm_sys::drm_mode_fb_cmd2;
use drm_sys::*;
use std::os::unix::prelude::RawFd;

ioctl_readwrite!(drm_mode_getfb2, DRM_IOCTL_BASE, 0xCE, drm_mode_fb_cmd2);

pub fn fb_cmd2(fd: RawFd, handle: u32) -> Result<drm_mode_fb_cmd2, SystemError> {
    let mut fb = drm_mode_fb_cmd2 {
        fb_id: handle,
        width: 0,
        height: 0,
        pixel_format: 0,
        flags: 0,
        handles: [0; 4],
        pitches: [0; 4],
        offsets: [0; 4],
        modifier: [0; 4],
    };

    unsafe {
        drm_mode_getfb2(fd, &mut fb)?;
    }

    Ok(fb)
}

ioctl_readwrite!(drm_prime_handle_to_fd, DRM_IOCTL_BASE, 0x2D, drm_prime_handle);

pub fn prime_handle_to_fd(fd: RawFd, handle: u32) -> Result<RawFd, SystemError> {
    let mut ph = drm_prime_handle {
        handle,
        // Never let the dma-buf fd leak into a child process.
        flags: libc::O_CLOEXEC as u32,
        fd: 0,
    };

    unsafe {
        drm_prime_handle_to_fd(fd, &mut ph)?;
    }

    Ok(ph.fd)
}

ioctl_write_ptr!(drm_gem_close_ioctl, DRM_IOCTL_BASE, 0x09, drm_gem_close);

pub fn gem_close(fd: RawFd, handle: u32) -> Result<(), SystemError> {
    let mut ph = drm_gem_close {
        handle,
        pad: 0,
    };

    unsafe {
        drm_gem_close_ioctl(fd, &mut ph)?;
    }

    Ok(())
}

// --- dma-buf CPU access synchronisation (linux/dma-buf.h) ---------------------
// Every CPU read of an mmap'd dma-buf must be bracketed by SYNC_START|READ and
// SYNC_END|READ (Documentation/driver-api/dma-buf.rst, "CPU access").
#[repr(C)]
pub struct dma_buf_sync {
    pub flags: u64,
}
pub const DMA_BUF_SYNC_READ: u64 = 1 << 0;
pub const DMA_BUF_SYNC_START: u64 = 0 << 2;
pub const DMA_BUF_SYNC_END: u64 = 1 << 2;

ioctl_write_ptr!(dma_buf_ioctl_sync, b'b', 0, dma_buf_sync);

/// Issue DMA_BUF_IOCTL_SYNC, retrying EINTR/EAGAIN a few times as the uapi
/// documentation asks. Returns the errno on persistent failure.
pub fn dmabuf_sync(fd: RawFd, flags: u64) -> Result<(), nix::errno::Errno> {
    let s = dma_buf_sync { flags };
    let mut tries = 0;
    loop {
        match unsafe { dma_buf_ioctl_sync(fd, &s) } {
            Ok(_) => return Ok(()),
            Err(nix::Error::Sys(e)) if (e == nix::errno::Errno::EINTR || e == nix::errno::Errno::EAGAIN) && tries < 3 => {
                tries += 1;
            }
            Err(nix::Error::Sys(e)) => return Err(e),
            Err(_) => return Err(nix::errno::Errno::EIO),
        }
    }
}
