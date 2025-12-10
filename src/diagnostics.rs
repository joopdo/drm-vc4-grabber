use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::collections::VecDeque;

#[derive(Clone)]
pub struct LogEntry {
    timestamp: u128,
    elapsed: u128,
    category: String,
    message: String,
}

pub struct DiagnosticLogger {
    writer: Arc<Mutex<BufWriter<File>>>,
    start_time: SystemTime,
    last_summary: Arc<Mutex<SystemTime>>,
    error_buffer: Arc<Mutex<VecDeque<LogEntry>>>,
    capture_count: Arc<Mutex<u64>>,
    last_hyperion_error: Arc<Mutex<Option<SystemTime>>>,
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
            last_summary: Arc::new(Mutex::new(start_time)),
            error_buffer: Arc::new(Mutex::new(VecDeque::with_capacity(100))),
            capture_count: Arc::new(Mutex::new(0)),
            last_hyperion_error: Arc::new(Mutex::new(None)),
        };
        
        // Log session start
        logger.log_immediate("SESSION", "=== DRM GRABBER DIAGNOSTIC SESSION START ===");
        logger.log_system_info();
        
        Ok(logger)
    }
    
    // Immediate logging for critical events
    pub fn log_immediate(&self, category: &str, message: &str) {
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
        
        // Print critical categories to stdout
        if matches!(category, "ERROR" | "WARN" | "SESSION" | "INIT" | "CRASH" | "SUMMARY") {
            print!("{}", log_line);
        }
    }
    
    // Buffered logging for regular events (only written during summaries or errors)
    pub fn log_buffered(&self, category: &str, message: &str) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
            
        let elapsed = SystemTime::now()
            .duration_since(self.start_time)
            .unwrap()
            .as_millis();
            
        let entry = LogEntry {
            timestamp,
            elapsed,
            category: category.to_string(),
            message: message.to_string(),
        };
        
        if let Ok(mut buffer) = self.error_buffer.lock() {
            buffer.push_back(entry);
            // Keep only last 100 entries
            if buffer.len() > 100 {
                buffer.pop_front();
            }
        }
    }
    
    // Smart logging - decides whether to log immediately or buffer
    pub fn log(&self, category: &str, message: &str) {
        match category {
            "ERROR" | "WARN" | "SESSION" | "INIT" | "CRASH" | "SUMMARY" => {
                self.log_immediate(category, message);
                // When we hit an error, dump the recent buffer
                if category == "ERROR" {
                    self.dump_error_context();
                }
            }
            _ => {
                self.log_buffered(category, message);
            }
        }
    }
    
    // Dump recent buffered events when an error occurs
    fn dump_error_context(&self) {
        if let Ok(buffer) = self.error_buffer.lock() {
            if !buffer.is_empty() {
                self.log_immediate("CONTEXT", "--- Recent events before error ---");
                for entry in buffer.iter() {
                    let log_line = format!("[{}] +{}ms [{}] {}", 
                                          entry.timestamp, entry.elapsed, 
                                          entry.category, entry.message);
                    if let Ok(mut writer) = self.writer.lock() {
                        let _ = writer.write_all(format!("{}\n", log_line).as_bytes());
                    }
                }
                self.log_immediate("CONTEXT", "--- End recent events ---");
            }
        }
    }
    
    // Periodic summary (every minute)
    pub fn maybe_log_summary(&self) {
        let now = SystemTime::now();
        let should_log = {
            let mut last_summary = self.last_summary.lock().unwrap();
            if now.duration_since(*last_summary).unwrap_or(Duration::ZERO) >= Duration::from_secs(60) {
                *last_summary = now;
                true
            } else {
                false
            }
        };
        
        if should_log {
            let capture_count = *self.capture_count.lock().unwrap();
            let elapsed_mins = now.duration_since(self.start_time)
                .unwrap_or(Duration::ZERO)
                .as_secs() / 60;
            
            self.log_immediate("SUMMARY", &format!("Running for {} minutes, {} captures completed", 
                                                  elapsed_mins, capture_count));
            
            // Log current system state
            self.log_system_summary();
        }
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
        if success {
            // Only log successful operations occasionally to reduce noise
            self.log_buffered("HYPERION", &format!("{} -> SUCCESS", operation));
        } else {
            // Deduplicate Hyperion errors - only log if it's been a while since last error
            let should_log = {
                let mut last_error = self.last_hyperion_error.lock().unwrap();
                let now = SystemTime::now();
                if let Some(last) = *last_error {
                    if now.duration_since(last).unwrap_or(Duration::ZERO) >= Duration::from_secs(10) {
                        *last_error = Some(now);
                        true
                    } else {
                        false
                    }
                } else {
                    *last_error = Some(now);
                    true
                }
            };
            
            if should_log {
                self.log_immediate("HYPERION_ERROR", &format!("{} -> FAILED ({})", operation, details));
            }
        }
    }
    
    pub fn log_capture_success(&self) {
        // Increment counter and maybe log summary
        {
            let mut count = self.capture_count.lock().unwrap();
            *count += 1;
        }
        
        // Check if we should log a periodic summary
        self.maybe_log_summary();
    }
    
    pub fn log_capture_milestone(&self, count: u64) {
        // Log every 500 captures to track progress without spam
        if count % 500 == 0 {
            self.log_immediate("MILESTONE", &format!("Completed {} captures", count));
        }
    }
    
    pub fn log_capture_error(&self, error: &str) {
        self.log("CAPTURE_ERROR", error);
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
            self.log_immediate("SYSTEM", &format!("Hostname: {}", hostname));
        }
        
        // Log process info
        self.log_immediate("SYSTEM", &format!("PID: {}", std::process::id()));
        
        // Log Rust/Cargo version info
        self.log_immediate("SYSTEM", "Built with Rust (version info not available)");
        
        // Log Kodi log path for monitoring
        self.log_immediate("SYSTEM", "Kodi log path: /storage/.kodi/temp/kodi.log");
    }
    
    fn log_system_summary(&self) {
        use std::process::Command;
        use std::fs;
        
        // Log current load and memory
        if let Ok(loadavg) = fs::read_to_string("/proc/loadavg") {
            let load = loadavg.split_whitespace().next().unwrap_or("unknown");
            self.log_immediate("SUMMARY", &format!("Load: {}", load));
        }
        
        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            let mut mem_total = 0;
            let mut mem_available = 0;
            
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    mem_total = Self::extract_kb_value(line);
                } else if line.starts_with("MemAvailable:") {
                    mem_available = Self::extract_kb_value(line);
                }
            }
            
            if mem_total > 0 {
                let mem_used_pct = ((mem_total - mem_available) * 100) / mem_total;
                self.log_immediate("SUMMARY", &format!("Memory: {}%", mem_used_pct));
            }
        }
        
        // Check Kodi status
        if let Ok(result) = Command::new("pgrep").arg("-f").arg("kodi").output() {
            if result.status.success() {
                let stdout_str = String::from_utf8_lossy(&result.stdout);
                let pid_count = stdout_str.trim().lines().count();
                self.log_immediate("SUMMARY", &format!("Kodi processes: {}", pid_count));
            } else {
                self.log_immediate("SUMMARY", "Kodi: NOT RUNNING");
            }
        }
        
        // Check for recent Kodi errors
        self.check_kodi_log_errors();
    }
    
    fn check_kodi_log_errors(&self) {
        use std::process::Command;
        
        let kodi_log_paths = [
            "/storage/.kodi/temp/kodi.log",
            "/var/log/kodi.log",
            "/home/osmc/.kodi/temp/kodi.log",
        ];
        
        for log_path in &kodi_log_paths {
            if let Ok(output) = Command::new("tail")
                .arg("-n")
                .arg("20")
                .arg(log_path)
                .output() {
                
                let log_content = String::from_utf8_lossy(&output.stdout);
                
                // Look for critical errors
                for line in log_content.lines() {
                    if line.contains("ERROR") && (
                        line.contains("drm") || 
                        line.contains("DRM") || 
                        line.contains("freeze") ||
                        line.contains("crash") ||
                        line.contains("segfault")
                    ) {
                        self.log_immediate("KODI_ERROR", &format!("From {}: {}", log_path, line));
                    }
                }
            }
        }
    }
    
    fn extract_kb_value(line: &str) -> u64 {
        line.split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
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