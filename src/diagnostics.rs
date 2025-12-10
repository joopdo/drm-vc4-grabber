use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};


pub struct DiagnosticLogger {
    writer: Arc<Mutex<BufWriter<File>>>,
    start_time: SystemTime,
}

impl DiagnosticLogger {
    pub fn new(log_path: &str) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;
        
        let writer = Arc::new(Mutex::new(BufWriter::new(file)));
        let start_time = SystemTime::now();
        
        let logger = DiagnosticLogger {
            writer,
            start_time,
        };
        
        // Log session start
        logger.log("SESSION", "=== DRM GRABBER DIAGNOSTIC SESSION START ===");
        logger.log_system_info();
        
        Ok(logger)
    }
    
    pub fn log(&self, category: &str, message: &str) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
            
        let elapsed = SystemTime::now()
            .duration_since(self.start_time)
            .unwrap()
            .as_millis();
            
        let log_line = format!("[{}] +{}ms [{}] {}\n", 
                              timestamp, elapsed, category, message);
        
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(log_line.as_bytes());
            let _ = writer.flush();
        }
        
        // Also print to stdout for immediate feedback
        print!("{}", log_line);
    }
    
    pub fn log_drm_operation(&self, operation: &str, handle: u32, result: &str) {
        self.log("DRM", &format!("{}(handle={}) -> {}", operation, handle, result));
    }
    
    pub fn log_resource_state(&self, prime_fds: usize, gem_handles: usize) {
        self.log("RESOURCE", &format!("Prime FDs: {}, GEM handles: {}", prime_fds, gem_handles));
    }
    
    pub fn log_framebuffer_info(&self, fb_handle: u32, size: (u32, u32), format: u32) {
        self.log("FB", &format!("handle={}, size={}x{}, format=0x{:x}", 
                               fb_handle, size.0, size.1, format));
    }
    
    pub fn log_hyperion_operation(&self, operation: &str, success: bool, details: &str) {
        let status = if success { "SUCCESS" } else { "FAILED" };
        self.log("HYPERION", &format!("{} -> {} ({})", operation, status, details));
    }
    
    pub fn log_error(&self, error: &str) {
        self.log("ERROR", error);
    }
    
    pub fn log_warning(&self, warning: &str) {
        self.log("WARN", warning);
    }
    
    fn log_system_info(&self) {
        // Log basic system information
        if let Ok(hostname) = std::env::var("HOSTNAME") {
            self.log("SYSTEM", &format!("Hostname: {}", hostname));
        }
        
        // Log process info
        self.log("SYSTEM", &format!("PID: {}", std::process::id()));
        
        // Log Rust/Cargo version info
        self.log("SYSTEM", "Built with Rust (version info not available)");
    }
}

pub struct ResourceTracker {
    logger: Arc<DiagnosticLogger>,
    prime_fds: Arc<Mutex<Vec<i32>>>,
    gem_handles: Arc<Mutex<Vec<u32>>>,
}

impl ResourceTracker {
    pub fn new(logger: Arc<DiagnosticLogger>) -> Self {
        ResourceTracker {
            logger,
            prime_fds: Arc::new(Mutex::new(Vec::new())),
            gem_handles: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    pub fn track_prime_fd(&self, fd: i32) {
        if let Ok(mut fds) = self.prime_fds.lock() {
            fds.push(fd);
            self.logger.log("TRACK", &format!("Prime FD {} allocated (total: {})", fd, fds.len()));
        }
    }
    
    pub fn untrack_prime_fd(&self, fd: i32) {
        if let Ok(mut fds) = self.prime_fds.lock() {
            if let Some(pos) = fds.iter().position(|&x| x == fd) {
                fds.remove(pos);
                self.logger.log("TRACK", &format!("Prime FD {} released (total: {})", fd, fds.len()));
            } else {
                self.logger.log_warning(&format!("Attempted to release untracked Prime FD {}", fd));
            }
        }
    }
    
    pub fn track_gem_handle(&self, handle: u32) {
        if let Ok(mut handles) = self.gem_handles.lock() {
            handles.push(handle);
            self.logger.log("TRACK", &format!("GEM handle {} allocated (total: {})", handle, handles.len()));
        }
    }
    
    pub fn untrack_gem_handle(&self, handle: u32) {
        if let Ok(mut handles) = self.gem_handles.lock() {
            if let Some(pos) = handles.iter().position(|&x| x == handle) {
                handles.remove(pos);
                self.logger.log("TRACK", &format!("GEM handle {} released (total: {})", handle, handles.len()));
            } else {
                self.logger.log_warning(&format!("Attempted to release untracked GEM handle {}", handle));
            }
        }
    }
    
    pub fn log_current_state(&self) {
        let prime_count = self.prime_fds.lock().map(|fds| fds.len()).unwrap_or(0);
        let gem_count = self.gem_handles.lock().map(|handles| handles.len()).unwrap_or(0);
        self.logger.log_resource_state(prime_count, gem_count);
    }
    
    pub fn check_for_leaks(&self) -> bool {
        let prime_count = self.prime_fds.lock().map(|fds| fds.len()).unwrap_or(0);
        let gem_count = self.gem_handles.lock().map(|handles| handles.len()).unwrap_or(0);
        
        if prime_count > 0 || gem_count > 0 {
            self.logger.log_warning(&format!("Resource leak detected: {} Prime FDs, {} GEM handles", 
                                            prime_count, gem_count));
            return true;
        }
        false
    }
}