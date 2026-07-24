//! `fabric-cli`: admin/debug tool for the running endpoint daemon.

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use fabric_classifier::{ClassifyInput, Complexity, Horizon, UserLocusPref};
use fabric_endpoint_cli::{
    print_decision, print_policy, print_sessions, print_status, DaemonClient,
};

#[derive(Parser)]
#[command(
    name = "fabric-cli",
    version,
    about = "Admin/debug CLI for the fabric endpoint daemon"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pretty-print daemon status (device, policy versions, sessions).
    Status,
    /// List active sessions from the context store.
    Sessions,
    /// Print the current effective policy summary.
    Policy,
    /// Probe daemon liveness.
    Health,
    /// Ask the daemon where a turn would run (endpoint/hosted/split).
    Classify {
        /// What the turn is trying to do.
        #[arg(long)]
        intent: String,
        /// Tools the turn needs, comma-separated.
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,
        /// Estimated complexity: low, medium, high.
        #[arg(long, default_value = "low")]
        complexity: String,
        /// Estimated horizon: single, multi, long.
        #[arg(long, default_value = "single")]
        horizon: String,
        /// Data classes touched, comma-separated (e.g. public,internal).
        #[arg(long, value_delimiter = ',')]
        data_classes: Vec<String>,
        /// Classify as if the network were down.
        #[arg(long)]
        offline: bool,
        /// Classify as if no local model were installed.
        #[arg(long)]
        no_local_model: bool,
        /// User locus preference: local, hosted, background, none.
        #[arg(long, default_value = "none")]
        prefer: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = DaemonClient::from_env();
    match cli.command {
        Command::Status => print_status(&client.status().await?),
        Command::Sessions => print_sessions(&client.sessions().await?),
        Command::Policy => print_policy(&client.policy().await?),
        Command::Health => match client.health().await {
            Ok(h) => println!("ok ({})", h.version),
            Err(e) => {
                println!("fail: {e:#}");
                std::process::exit(1);
            }
        },
        Command::Classify {
            intent,
            tools,
            complexity,
            horizon,
            data_classes,
            offline,
            no_local_model,
            prefer,
        } => {
            let input = ClassifyInput {
                intent_text: intent,
                required_tools: tools,
                estimated_complexity: parse_complexity(&complexity)?,
                estimated_horizon: parse_horizon(&horizon)?,
                data_classes,
                network_available: !offline,
                local_model_available: !no_local_model,
                user_preference: parse_preference(&prefer)?,
            };
            print_decision(&client.classify(&input).await?);
        }
    }
    Ok(())
}

fn parse_complexity(s: &str) -> Result<Complexity> {
    match s {
        "low" => Ok(Complexity::Low),
        "medium" => Ok(Complexity::Medium),
        "high" => Ok(Complexity::High),
        other => bail!("invalid complexity '{other}' (expected low|medium|high)"),
    }
}

fn parse_horizon(s: &str) -> Result<Horizon> {
    match s {
        "single" => Ok(Horizon::SingleTurn),
        "multi" => Ok(Horizon::MultiTurn),
        "long" => Ok(Horizon::LongHorizon),
        other => bail!("invalid horizon '{other}' (expected single|multi|long)"),
    }
}

fn parse_preference(s: &str) -> Result<UserLocusPref> {
    match s {
        "local" => Ok(UserLocusPref::PreferLocal),
        "hosted" => Ok(UserLocusPref::PreferHosted),
        "background" => Ok(UserLocusPref::Background),
        "none" => Ok(UserLocusPref::NoPreference),
        other => bail!("invalid preference '{other}' (expected local|hosted|background|none)"),
    }
}
