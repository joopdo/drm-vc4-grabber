use std::fs;
use std::process::Command;
use std::time::Duration;
use std::thread;
use std::sync::{Arc, Mutex};
use crate::diagnostics::DiagnosticLogger;

pub struct SystemMonitor {
    logger: Arc<DiagnosticLogger>,
    monitoring: Arc<Mutex<bool>>,
}

impl SystemMonitor {
    pub fn new(logger: Arc<DiagnosticLogger>) -> Self {
        SystemMonitor {
            logger,
            monitoring: Arc::new(Mutex::new(false)),
        }
    }
    
    pub fn start_monitoring(&self, interval_ms: u64) {
        {
            let mut monitoring = self.monitoring.lock().unwrap();
            if *monitoring {
                self.logger.log_warning("System monitoring already running");
                return;
            }
            *monitoring = true;
        }
        
        let logger = Arc::clone(&self.logger);
        let monitoring = Arc::clone(&self.monitoring);
        
        thread::spawn(move || {
            logger.log("MONITOR", "System monitoring started");
            
            while *monitoring.lock().unwrap() {
                Self::collect_system_metrics(&logger);
                Self::check_kodi_status(&logger);
                Self::check_drm_status(&logger);
                Self::check_memory_pressure(&logger);
                
                thread::sleep(Duration::from_millis(interval_ms));
            }
            
            logger.log("MONITOR", "System monitoring stopped");
        });
    }
    
    pub fn stop_monitoring(&self) {
        let mut monitoring = self.monitoring.lock().unwrap();
        *monitoring = false;
        self.logger.log("MONITOR", "System monitoring stop requested");
    }
    
    fn collect_system_metrics(logger: &DiagnosticLogger) {
        // CPU usage
        if let Ok(loadavg) = fs::read_to_string("/proc/loadavg") {
            let load = loadavg.split_whitespace().next().unwrap_or("unknown");
            logger.log("SYSTEM", &format!("Load average: {}", load));
        }
        
        // Memory usage
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
                logger.log("SYSTEM", &format!("Memory usage: {}% ({}/{} MB)", 
                                             mem_used_pct, 
                                             (mem_total - mem_available) / 1024,
                                             mem_total / 1024));
            }
        }
    }
    
    fn check_kodi_status(logger: &DiagnosticLogger) {
        // Check if Kodi is running
        let output = Command::new("pgrep")
            .arg("-f")
            .arg("kodi")
            .output();
            
        match output {
            Ok(result) if result.status.success() => {
                let stdout_str = String::from_utf8_lossy(&result.stdout);
                let pids: Vec<&str> = stdout_str
                    .trim()
                    .lines()
                    .collect();
                logger.log("KODI", &format!("Running (PIDs: {})", pids.join(", ")));
                
                // Check Kodi's resource usage
                for pid in &pids {
                    Self::check_process_resources(logger, pid, "kodi");
                }
            }
            _ => {
                logger.log("KODI", "Not running");
            }
        }
        
        // Check Kodi logs for recent errors
        Self::check_kodi_logs(logger);
    }
    
    fn check_kodi_logs(logger: &DiagnosticLogger) {
        // Check for recent Kodi crashes or DRM errors
        let log_paths = [
            "/var/log/kodi.log",
            "/home/osmc/.kodi/temp/kodi.log",
            "/storage/.kodi/temp/kodi.log",
        ];
        
        for log_path in &log_paths {
            if let Ok(output) = Command::new("tail")
                .arg("-n")
                .arg("10")
                .arg(log_path)
                .output() {
                
                let log_content = String::from_utf8_lossy(&output.stdout);
                if log_content.contains("ERROR") || log_content.contains("drm") || log_content.contains("DRM") {
                    logger.log("KODI_LOG", &format!("Recent errors in {}: {}", 
                                                   log_path, 
                                                   log_content.lines().last().unwrap_or("unknown")));
                }
            }
        }
    }
    
    fn check_drm_status(logger: &DiagnosticLogger) {
        // Check DRM clients
        if let Ok(clients) = fs::read_to_string("/sys/kernel/debug/dri/0/clients") {
            let client_count = clients.lines().count().saturating_sub(1); // Subtract header
            logger.log("DRM", &format!("Active clients: {}", client_count));
            
            // Log client details if verbose
            for (i, line) in clients.lines().enumerate() {
                if i > 0 && i <= 5 { // Skip header, show first 5 clients
                    logger.log("DRM_CLIENT", line);
                }
            }
        }
        
        // Check GEM objects
        if let Ok(gem_names) = fs::read_to_string("/sys/kernel/debug/dri/0/gem_names") {
            let gem_count = gem_names.lines().count().saturating_sub(1);
            logger.log("DRM", &format!("GEM objects: {}", gem_count));
            
            if gem_count > 100 {
                logger.log_warning(&format!("High GEM object count: {}", gem_count));
            }
        }
        
        // Check for DRM errors in dmesg
        if let Ok(output) = Command::new("dmesg")
            .arg("-T")
            .arg("--since")
            .arg("1 minute ago")
            .output() {
            
            let dmesg_content = String::from_utf8_lossy(&output.stdout);
            for line in dmesg_content.lines() {
                if line.contains("drm") || line.contains("vc4") {
                    if line.contains("ERROR") || line.contains("WARN") || line.contains("failed") {
                        logger.log("DRM_ERROR", line);
                    }
                }
            }
        }
    }
    
    fn check_memory_pressure(logger: &DiagnosticLogger) {
        // Check for OOM killer activity
        if let Ok(output) = Command::new("dmesg")
            .arg("-T")
            .arg("--since")
            .arg("1 minute ago")
            .output() {
            
            let dmesg_content = String::from_utf8_lossy(&output.stdout);
            for line in dmesg_content.lines() {
                if line.contains("Out of memory") || line.contains("oom-killer") {
                    logger.log("OOM", line);
                }
            }
        }
        
        // Check memory pressure indicators
        if let Ok(pressure) = fs::read_to_string("/proc/pressure/memory") {
            for line in pressure.lines() {
                if line.starts_with("full") {
                    logger.log("MEMORY_PRESSURE", line);
                }
            }
        }
    }
    
    fn check_process_resources(logger: &DiagnosticLogger, pid: &str, process_name: &str) {
        // Check process memory usage
        if let Ok(status) = fs::read_to_string(format!("/proc/{}/status", pid)) {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    let rss_kb = Self::extract_kb_value(line);
                    logger.log("PROC", &format!("{} (PID {}): RSS {} MB", 
                                               process_name, pid, rss_kb / 1024));
                }
            }
        }
        
        // Check file descriptor usage
        if let Ok(fd_dir) = fs::read_dir(format!("/proc/{}/fd", pid)) {
            let fd_count = fd_dir.count();
            logger.log("PROC", &format!("{} (PID {}): {} file descriptors", 
                                       process_name, pid, fd_count));
            
            if fd_count > 500 {
                logger.log_warning(&format!("{} has high FD count: {}", process_name, fd_count));
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