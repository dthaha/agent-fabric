//! Endpoint CLI: admin/debug tooling for the endpoint daemon — inspect
//! sessions, leases, and the op-log; force handoffs; verify seeding state.
//!
//! Talks to the daemon's localhost health/status HTTP server.

use anyhow::{Context, Result};
use fabric_classifier::{ClassifyInput, LocusDecision};
use serde::Deserialize;

/// Default port for the daemon's localhost HTTP server.
pub const DEFAULT_HEALTH_PORT: u16 = 47770;

/// Base URL of the running daemon: `FABRIC_HEALTH_PORT` env override, else
/// the default port.
pub fn daemon_base_url() -> String {
    let port = std::env::var("FABRIC_HEALTH_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_HEALTH_PORT);
    format!("http://127.0.0.1:{port}")
}

#[derive(Debug, Deserialize)]
pub struct Health {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct Status {
    pub device_id: String,
    pub version: String,
    pub uptime_secs: u64,
    pub policy_endpoint_version: String,
    pub policy_server_version: String,
    pub context_db_path: String,
    pub server_url: String,
    pub active_sessions: u64,
    pub tool_bridge_port: u16,
}

#[derive(Debug, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub state: String,
    pub created_at: i64,
    pub last_entry_seq: u64,
}

#[derive(Debug, Deserialize)]
pub struct PolicyInfo {
    pub endpoint_version: String,
    pub server_version: String,
    pub tool_rule_count: usize,
    pub kill_switch: bool,
    pub cua_enabled: bool,
}

/// HTTP client for the daemon's localhost admin API.
pub struct DaemonClient {
    base: String,
    http: reqwest::Client,
}

impl DaemonClient {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            http: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(daemon_base_url())
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        self.http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .with_context(|| format!("connecting to daemon at {}", self.base))?
            .error_for_status()
            .with_context(|| format!("daemon returned an error for {path}"))?
            .json()
            .await
            .with_context(|| format!("decoding daemon response for {path}"))
    }

    pub async fn health(&self) -> Result<Health> {
        self.get("/healthz").await
    }

    pub async fn status(&self) -> Result<Status> {
        self.get("/status").await
    }

    pub async fn sessions(&self) -> Result<Vec<SessionInfo>> {
        self.get("/sessions").await
    }

    pub async fn policy(&self) -> Result<PolicyInfo> {
        self.get("/policy").await
    }

    /// Ask the daemon to classify a turn's locus.
    pub async fn classify(&self, input: &ClassifyInput) -> Result<LocusDecision> {
        self.http
            .post(format!("{}/classify", self.base))
            .json(input)
            .send()
            .await
            .with_context(|| format!("connecting to daemon at {}", self.base))?
            .error_for_status()
            .context("daemon returned an error for /classify")?
            .json()
            .await
            .context("decoding daemon response for /classify")
    }
}

pub fn print_status(s: &Status) {
    let server = if s.server_url.is_empty() {
        "(offline-only)"
    } else {
        &s.server_url
    };
    println!("device:            {}", s.device_id);
    println!("version:           {}", s.version);
    println!("uptime:            {}s", s.uptime_secs);
    println!(
        "policy (endpoint): {}",
        none_if_empty(&s.policy_endpoint_version)
    );
    println!(
        "policy (server):   {}",
        none_if_empty(&s.policy_server_version)
    );
    println!("context db:        {}", s.context_db_path);
    println!("server:            {server}");
    println!("active sessions:   {}", s.active_sessions);
    println!("tool bridge port:  {}", s.tool_bridge_port);
}

pub fn print_sessions(sessions: &[SessionInfo]) {
    if sessions.is_empty() {
        println!("no active sessions");
        return;
    }
    println!(
        "{:<40} {:<14} {:>16} {:>6}",
        "SESSION", "STATE", "CREATED (ms)", "SEQ"
    );
    for s in sessions {
        println!(
            "{:<40} {:<14} {:>16} {:>6}",
            s.session_id, s.state, s.created_at, s.last_entry_seq
        );
    }
}

pub fn print_policy(p: &PolicyInfo) {
    println!("endpoint version: {}", none_if_empty(&p.endpoint_version));
    println!("server version:   {}", none_if_empty(&p.server_version));
    println!("tool rules:       {}", p.tool_rule_count);
    println!("kill switch:      {}", on_off(p.kill_switch));
    println!("cua enabled:      {}", on_off(p.cua_enabled));
}

pub fn print_decision(d: &LocusDecision) {
    println!("locus:      {}", d.locus.as_str_name());
    println!("reason:     {}", d.reason);
    println!("confidence: {:.2}", d.confidence);
    println!(
        "fallback:   {}",
        d.fallback
            .map(|l| l.as_str_name().to_string())
            .unwrap_or_else(|| "(none)".into())
    );
}

fn none_if_empty(s: &str) -> &str {
    if s.is_empty() {
        "(none)"
    } else {
        s
    }
}

fn on_off(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}
