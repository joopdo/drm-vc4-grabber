use std::net::TcpStream;
use std::time::{Duration, Instant};
use std::io::Result as StdResult;
use std::sync::Arc;

use crate::hyperion::{register_direct, read_reply, send_color_warm, send_image};
use crate::diagnostics::DiagnosticLogger;

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub address: String,
    pub max_retries: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub connection_timeout_ms: u64,
    pub health_check_interval_ms: u64,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        ConnectionConfig {
            address: "127.0.0.1:19400".to_string(),
            max_retries: 5,
            initial_backoff_ms: 100,
            max_backoff_ms: 5000,
            connection_timeout_ms: 3000,
            health_check_interval_ms: 30000, // 30 seconds
        }
    }
}

#[derive(Debug)]
pub enum ConnectionState {
    Connected,
    Disconnected,
    Reconnecting,
    Failed,
}

pub struct HyperionConnectionManager {
    config: ConnectionConfig,
    socket: Option<TcpStream>,
    state: ConnectionState,
    last_connection_attempt: Option<Instant>,
    consecutive_failures: u32,
    logger: Option<Arc<DiagnosticLogger>>,
    last_health_check: Instant,
    total_reconnections: u32,
    connection_start_time: Option<Instant>,
}

impl HyperionConnectionManager {
    pub fn new(config: ConnectionConfig, logger: Option<Arc<DiagnosticLogger>>) -> Self {
        println!("Creating HyperionConnectionManager for {}", config.address);
        if let Some(ref logger) = logger {
            logger.log("HYPERION", &format!("Connection manager initialized for {}", config.address));
        } else {
            println!("No logger provided to connection manager");
        }
        
        HyperionConnectionManager {
            config,
            socket: None,
            state: ConnectionState::Disconnected,
            last_connection_attempt: None,
            consecutive_failures: 0,
            logger,
            last_health_check: Instant::now(),
            total_reconnections: 0,
            connection_start_time: None,
        }
    }
    
    /// Ensure we have a healthy connection, reconnecting if necessary
    pub fn ensure_connected(&mut self) -> StdResult<&mut TcpStream> {
        // Check if we need to perform a health check
        if self.last_health_check.elapsed() >= Duration::from_millis(self.config.health_check_interval_ms) {
            self.perform_health_check();
            self.last_health_check = Instant::now();
        }
        
        match self.state {
            ConnectionState::Connected => {
                if let Some(ref mut socket) = self.socket {
                    return Ok(socket);
                } else {
                    // Inconsistent state - fix it
                    self.state = ConnectionState::Disconnected;
                }
            }
            ConnectionState::Disconnected | ConnectionState::Failed => {
                self.attempt_connection()?;
            }
            ConnectionState::Reconnecting => {
                // Check if enough time has passed for retry
                if let Some(last_attempt) = self.last_connection_attempt {
                    let backoff_duration = self.calculate_backoff_duration();
                    if last_attempt.elapsed() >= backoff_duration {
                        self.attempt_connection()?;
                    } else {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::NotConnected,
                            "Still in backoff period"
                        ));
                    }
                } else {
                    self.attempt_connection()?;
                }
            }
        }
        
        // If we get here, we should have a connection
        if let Some(ref mut socket) = self.socket {
            Ok(socket)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Failed to establish connection"
            ))
        }
    }
    
    /// Attempt to establish a new connection
    fn attempt_connection(&mut self) -> StdResult<()> {
        self.state = ConnectionState::Reconnecting;
        self.last_connection_attempt = Some(Instant::now());
        
        if let Some(ref logger) = self.logger {
            logger.log("HYPERION", &format!("Attempting connection to {} (attempt {} of {})", 
                                           self.config.address, 
                                           self.consecutive_failures + 1, 
                                           self.config.max_retries));
        }
        
        match TcpStream::connect(&self.config.address) {
            Ok(mut socket) => {
                // Set socket timeout
                socket.set_read_timeout(Some(Duration::from_millis(self.config.connection_timeout_ms)))?;
                socket.set_write_timeout(Some(Duration::from_millis(self.config.connection_timeout_ms)))?;
                
                // Perform Hyperion handshake
                match self.perform_handshake(&mut socket) {
                    Ok(()) => {
                        self.socket = Some(socket);
                        self.state = ConnectionState::Connected;
                        self.consecutive_failures = 0;
                        self.connection_start_time = Some(Instant::now());
                        
                        if self.total_reconnections > 0 {
                            if let Some(ref logger) = self.logger {
                                logger.log("HYPERION", &format!("Successfully reconnected to Hyperion (reconnection #{})", 
                                                               self.total_reconnections));
                            }
                        } else {
                            if let Some(ref logger) = self.logger {
                                logger.log("HYPERION", "Successfully connected to Hyperion");
                            }
                        }
                        
                        Ok(())
                    }
                    Err(e) => {
                        self.handle_connection_failure(format!("Handshake failed: {}", e));
                        Err(e)
                    }
                }
            }
            Err(e) => {
                self.handle_connection_failure(format!("TCP connection failed: {}", e));
                Err(e)
            }
        }
    }
    
    /// Perform Hyperion protocol handshake
    fn perform_handshake(&self, socket: &mut TcpStream) -> StdResult<()> {
        // Register with Hyperion
        register_direct(socket)?;
        read_reply(socket, false)?;
        
        // Send initial warm color to verify connection works
        send_color_warm(socket, false)?;
        
        Ok(())
    }
    
    /// Handle connection failure with exponential backoff
    fn handle_connection_failure(&mut self, error_msg: String) {
        self.consecutive_failures += 1;
        self.socket = None;
        
        if self.consecutive_failures >= self.config.max_retries {
            self.state = ConnectionState::Failed;
            if let Some(ref logger) = self.logger {
                logger.log_error(&format!("Hyperion connection failed permanently after {} attempts: {}", 
                                        self.consecutive_failures, error_msg));
            }
        } else {
            self.state = ConnectionState::Reconnecting;
            let backoff_duration = self.calculate_backoff_duration();
            
            if let Some(ref logger) = self.logger {
                logger.log_warning(&format!("Hyperion connection failed (attempt {} of {}): {}. Retrying in {}ms", 
                                          self.consecutive_failures, 
                                          self.config.max_retries,
                                          error_msg,
                                          backoff_duration.as_millis()));
            }
        }
    }
    
    /// Calculate exponential backoff duration
    fn calculate_backoff_duration(&self) -> Duration {
        let backoff_ms = std::cmp::min(
            self.config.initial_backoff_ms * (2_u64.pow(self.consecutive_failures.saturating_sub(1))),
            self.config.max_backoff_ms
        );
        Duration::from_millis(backoff_ms)
    }
    
    /// Handle network errors during operation
    pub fn handle_network_error(&mut self, error: &std::io::Error) -> bool {
        let should_reconnect = match error.kind() {
            std::io::ErrorKind::BrokenPipe |
            std::io::ErrorKind::ConnectionAborted |
            std::io::ErrorKind::ConnectionReset |
            std::io::ErrorKind::UnexpectedEof => true,
            _ => false,
        };
        
        if should_reconnect {
            if let Some(ref logger) = self.logger {
                logger.log_warning(&format!("Network error detected: {}. Marking connection for reconnection.", error));
            }
            
            self.socket = None;
            self.state = ConnectionState::Disconnected;
            self.total_reconnections += 1;
            
            // Reset consecutive failures for network errors (not connection failures)
            self.consecutive_failures = 0;
        }
        
        should_reconnect
    }
    
    /// Perform periodic health check
    fn perform_health_check(&mut self) {
        if let ConnectionState::Connected = self.state {
            if let Some(ref socket) = self.socket {
                // Simple check - try to get socket peer address
                match socket.peer_addr() {
                    Ok(_) => {
                        // Connection appears healthy
                        if let Some(ref logger) = self.logger {
                            if let Some(start_time) = self.connection_start_time {
                                let uptime = start_time.elapsed();
                                logger.log("HYPERION", &format!("Connection healthy (uptime: {}s)", uptime.as_secs()));
                            }
                        }
                    }
                    Err(_) => {
                        // Connection is broken
                        if let Some(ref logger) = self.logger {
                            logger.log_warning("Health check failed - connection appears broken");
                        }
                        self.socket = None;
                        self.state = ConnectionState::Disconnected;
                    }
                }
            }
        }
    }
    
    /// Get connection statistics
    pub fn get_stats(&self) -> ConnectionStats {
        ConnectionStats {
            state: format!("{:?}", self.state),
            consecutive_failures: self.consecutive_failures,
            total_reconnections: self.total_reconnections,
            uptime_seconds: self.connection_start_time
                .map(|start| start.elapsed().as_secs())
                .unwrap_or(0),
            last_attempt_ago_ms: self.last_connection_attempt
                .map(|last| last.elapsed().as_millis() as u64)
                .unwrap_or(0),
        }
    }
    
    /// Check if connection is available for use
    pub fn is_connected(&self) -> bool {
        matches!(self.state, ConnectionState::Connected) && self.socket.is_some()
    }
    
    /// Check if we're in a failed state (no more retries)
    pub fn is_failed(&self) -> bool {
        matches!(self.state, ConnectionState::Failed)
    }
    
    /// Reset failure state (useful for manual recovery or service restart detection)
    pub fn reset_failure_state(&mut self) {
        if matches!(self.state, ConnectionState::Failed) {
            if let Some(ref logger) = self.logger {
                logger.log("HYPERION", "Resetting failure state - will attempt reconnection");
            }
            self.state = ConnectionState::Disconnected;
            self.consecutive_failures = 0;
        }
    }
    
    /// Force reconnection (useful for testing or manual recovery)
    pub fn force_reconnect(&mut self) {
        if let Some(ref logger) = self.logger {
            logger.log("HYPERION", "Forcing reconnection");
        }
        
        self.socket = None;
        self.state = ConnectionState::Disconnected;
        self.consecutive_failures = 0;
    }
    
    /// Try to send image with fallback behavior
    pub fn send_image_with_fallback(&mut self, img: &image::RgbImage, verbose: bool) -> Result<bool, std::io::Error> {
        match self.ensure_connected() {
            Ok(socket) => {
                match send_image(socket, img, verbose) {
                    Ok(()) => {
                        if let Some(ref logger) = self.logger {
                            logger.log_hyperion_operation("send_image", true, "");
                        }
                        Ok(true) // Successfully sent
                    }
                    Err(e) => {
                        if self.handle_network_error(&e) {
                            if let Some(ref logger) = self.logger {
                                logger.log_hyperion_operation("send_image", false, &format!("Network error, will retry: {}", e));
                            }
                            Ok(false) // Failed but will retry
                        } else {
                            if let Some(ref logger) = self.logger {
                                logger.log_hyperion_operation("send_image", false, &e.to_string());
                            }
                            Err(e)
                        }
                    }
                }
            }
            Err(e) => {
                if self.is_failed() {
                    // In permanent failure state - operate in fallback mode
                    if let Some(ref logger) = self.logger {
                        logger.log_hyperion_operation("fallback", true, "Operating without Hyperion connection");
                    }
                    Ok(false) // Fallback mode - continue without Hyperion
                } else {
                    if let Some(ref logger) = self.logger {
                        logger.log_hyperion_operation("connection", false, &format!("Connection unavailable: {}", e));
                    }
                    Ok(false) // Temporary failure - will retry
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct ConnectionStats {
    pub state: String,
    pub consecutive_failures: u32,
    pub total_reconnections: u32,
    pub uptime_seconds: u64,
    pub last_attempt_ago_ms: u64,
}