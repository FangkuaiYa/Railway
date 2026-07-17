//! Server configuration.
//!
//! Configuration is loaded from:
//! 1. Default values
//! 2. `config.toml` file (optional)
//! 3. `IMPOSTOR_*` environment variables
//! 4. CLI arguments

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Main server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_listen_ip")]
    pub listen_ip: String,

    #[serde(default = "default_listen_port")]
    pub listen_port: u16,

    #[serde(default = "default_public_ip")]
    pub public_ip: String,

    #[serde(default = "default_public_port")]
    pub public_port: u16,

    #[serde(default)]
    pub anticheat: AntiCheatConfig,

    #[serde(default)]
    pub compatibility: CompatibilityConfig,

    #[serde(default)]
    pub http: HttpConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_ip: default_listen_ip(),
            listen_port: default_listen_port(),
            public_ip: default_public_ip(),
            public_port: default_public_port(),
            anticheat: AntiCheatConfig::default(),
            compatibility: CompatibilityConfig::default(),
            http: HttpConfig::default(),
        }
    }
}

impl ServerConfig {
    /// Load configuration from file, env, and defaults.
    pub fn load(config_path: Option<&str>) -> Result<Self, config::ConfigError> {
        let path = config_path.unwrap_or("config.toml");

        let cfg = config::Config::builder()
            .add_source(config::File::with_name(path).required(false))
            .add_source(
                config::Environment::with_prefix("IMPOSTOR")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        cfg.try_deserialize()
    }

    /// Get the listen socket address.
    pub fn listen_addr(&self) -> SocketAddr {
        format!("{}:{}", self.listen_ip, self.listen_port)
            .parse()
            .unwrap_or_else(|_| "0.0.0.0:22023".parse().unwrap())
    }
}

/// Anti-cheat configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiCheatConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_true")]
    pub ban_ip_from_game: bool,

    #[serde(default)]
    pub allow_cheating_hosts: CheatingHostMode,

    #[serde(default)]
    pub allow_host_only_extensions: CheatingHostMode,

    #[serde(default = "default_true")]
    pub enable_game_flow_checks: bool,

    #[serde(default = "default_true")]
    pub enable_must_be_host_checks: bool,

    #[serde(default = "default_true")]
    pub enable_color_limit_checks: bool,

    #[serde(default = "default_true")]
    pub enable_name_limit_checks: bool,

    #[serde(default = "default_true")]
    pub enable_ownership_checks: bool,

    #[serde(default = "default_true")]
    pub enable_packet_size_checks: bool,

    #[serde(default = "default_packet_size_limit")]
    pub packet_size_limit: usize,
}

impl Default for AntiCheatConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ban_ip_from_game: true,
            allow_cheating_hosts: CheatingHostMode::Never,
            allow_host_only_extensions: CheatingHostMode::IfRequested,
            enable_game_flow_checks: true,
            enable_must_be_host_checks: true,
            enable_color_limit_checks: true,
            enable_name_limit_checks: true,
            enable_ownership_checks: true,
            enable_packet_size_checks: true,
            packet_size_limit: 1203,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum CheatingHostMode {
    Never,
    #[default]
    IfRequested,
    Always,
}

/// Compatibility configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityConfig {
    #[serde(default)]
    pub allow_future_game_versions: bool,

    #[serde(default)]
    pub allow_host_authority: bool,

    #[serde(default)]
    pub allow_version_mixing: bool,
}

impl Default for CompatibilityConfig {
    fn default() -> Self {
        Self {
            allow_future_game_versions: false,
            allow_host_authority: false,
            allow_version_mixing: false,
        }
    }
}

/// HTTP API configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_http_ip")]
    pub listen_ip: String,

    #[serde(default = "default_http_port")]
    pub listen_port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen_ip: "0.0.0.0".into(),
            listen_port: 22023,
        }
    }
}

// Default value helpers
fn default_listen_ip() -> String { "0.0.0.0".into() }
fn default_listen_port() -> u16 { 22023 }
fn default_public_ip() -> String { "127.0.0.1".into() }
fn default_public_port() -> u16 { 22023 }
fn default_true() -> bool { true }
fn default_packet_size_limit() -> usize { 1203 }
fn default_http_ip() -> String { "0.0.0.0".into() }
fn default_http_port() -> u16 { 8080 }
