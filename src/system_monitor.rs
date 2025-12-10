use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::fs;
use std::process::Command;

use crate::diagnostics::DiagnosticLogger;

pub struct SystemMonitor {
    logger: Arc<DiagnosticLogger>,
}

impl SystemMonitor {
    pub fn new(logger: Arc<DiagnosticLogger>) -> Self {
        SystemMonitor { logger }
    }
    
    pub fn start_monitoring(&self, interval_ms: u64) {
        let logger = Arc::clone(&self.logger);
        
        thread::spawn(move || {
            logger.log("MONITOR", "System monitoring started");
            
            loop {
                Self::collect_system_metrics(&logger);
                thread::sleep(Duration::from_millis(interval_ms));
            }
        });
    }
    
    fn collect_system_metrics(logger: &Arc<DiagnosticLogger>) {
        // CPU Load Average
        if let Ok(loadavg) = fs::read_to_string("/proc/loadavg") {
            let parts: Vec<&str> = loadavg.split_whitespace().collect();
            if parts.len() >= 3 {
                logger.log("MONITOR", &format!("Load: {} {} {} (1m 5m 15m)", 
                                             parts[0], parts[1], parts[2]));
            }
        }
        
        // Memory Usage
        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            let mut mem_total = 0u64;
            let mut mem_available = 0u64;
            let mut mem_free = 0u64;
            let mut buffers = 0u64;
            let mut cached = 0u64;
            
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    mem_total = Self::extract_kb_value(line);
                } else if line.starts_with("MemAvailable:") {
                    mem_available = Self::extract_kb_value(line);
                } else if line.starts_with("MemFree:") {
                    mem_free = Self::extract_kb_value(line);
                } else if line.starts_with("Buffers:") {
                    buffers = Self::extract_kb_value(line);
                } else if line.starts_with("Cached:") {
                    cached = Self::extract_kb_value(line);
                }
            }
            
            if mem_total > 0 {
                let mem_used = mem_total - mem_available;
                let mem_used_pct = (mem_used * 100) / mem_total;
                logger.log("MONITOR", &format!("Memory: {}% used ({} MB / {} MB)", 
                                             mem_used_pct, 
                                             mem_used / 1024, 
                                             mem_total / 1024));
            }
        }
        
        // CPU Temperature (Pi-specific)
        if let Ok(temp_str) = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp") {
            if let Ok(temp_millic) = temp_str.trim().parse::<u32>() {
                let temp_c = temp_millic / 1000;
                logger.log("MONITOR", &format!("CPU Temperature: {}°C", temp_c));
                
                // Warn if temperature is high
                if temp_c > 70 {
                    logger.log_warning(&format!("High CPU temperature: {}°C", temp_c));
                }
            }
        }
        
        // Disk Usage for /storage (LibreELEC)
        if let Ok(output) = Command::new("df").arg("-h").arg("/storage").output() {
            let df_output = String::from_utf8_lossy(&output.stdout);
            for line in df_output.lines().skip(1) { // Skip header
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    logger.log("MONITOR", &format!("Storage: {} used, {} available ({})", 
                                                 parts[2], parts[3], parts[4]));
                }
                break; // Only first data line
            }
        }
        
        // Process monitoring
        Self::monitor_processes(logger);
        
        // DRM resource monitoring
        Self::monitor_drm_resources(logger);
    }
    
    fn monitor_processes(logger: &Arc<DiagnosticLogger>) {
        // Monitor Kodi processes
        if let Ok(output) = Command::new("pgrep").arg("-f").arg("kodi").output() {
            if output.status.success() {
                let stdout_str = String::from_utf8_lossy(&output.stdout);
                let pid_count = stdout_str.trim().lines().count();
                
                if pid_count > 0 {
                    // Get memory usage for Kodi processes
                    if let Ok(ps_output) = Command::new("ps")
                        .args(&["aux"])
                        .output() {
                        
                        let ps_str = String::from_utf8_lossy(&ps_output.stdout);
                        let mut kodi_mem_total = 0.0;
                        let mut kodi_cpu_total = 0.0;
                        
                        for line in ps_str.lines() {
                            if line.contains("kodi") {
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if parts.len() > 5 {
                                    if let Ok(cpu) = parts[2].parse::<f32>() {
                                        kodi_cpu_total += cpu;
                                    }
                                    if let Ok(mem) = parts[3].parse::<f32>() {
                                        kodi_mem_total += mem;
                                    }
                                }
                            }
                        }
                        
                        logger.log("MONITOR", &format!("Kodi: {} processes, {:.1}% CPU, {:.1}% Memory", 
                                                     pid_count, kodi_cpu_total, kodi_mem_total));
                    }
                } else {
                    logger.log("MONITOR", "Kodi: Not running");
                }
            }
        }
        
        // Monitor our own process
        let our_pid = std::process::id();
        if let Ok(output) = Command::new("ps")
            .args(&["-p", &our_pid.to_string(), "-o", "pid,pcpu,pmem,vsz,rss"])
            .output() {
            
            let ps_output = String::from_utf8_lossy(&output.stdout);
            for line in ps_output.lines().skip(1) { // Skip header
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    logger.log("MONITOR", &format!("Grabber: {}% CPU, {}% Memory, {} KB VSZ, {} KB RSS", 
                                                 parts[1], parts[2], parts[3], parts[4]));
                }
                break;
            }
        }
    }
    
    fn monitor_drm_resources(logger: &Arc<DiagnosticLogger>) {
        // Check DRM clients
        if let Ok(clients) = fs::read_to_string("/sys/kernel/debug/dri/0/clients") {
            let client_count = clients.lines().skip(1).count(); // Skip header
            logger.log("MONITOR", &format!("DRM clients: {}", client_count));
        }
        
        // Check GEM objects (if available)
        if let Ok(gem_names) = fs::read_to_string("/sys/kernel/debug/dri/0/gem_names") {
            let gem_count = gem_names.lines().skip(1).count(); // Skip header
            logger.log("MONITOR", &format!("GEM objects: {}", gem_count));
        }
        
        // Check for DRM errors in kernel log
        if let Ok(output) = Command::new("dmesg")
            .args(&["-T", "--level=err", "--since", "1 minute ago"])
            .output() {
            
            let dmesg_output = String::from_utf8_lossy(&output.stdout);
            for line in dmesg_output.lines() {
                if line.contains("drm") || line.contains("DRM") {
                    logger.log_warning(&format!("Kernel DRM error: {}", line));
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