//! `fabric-cli`: admin/debug tool for the running endpoint daemon.

use anyhow::Result;
use clap::{Parser, Subcommand};
use fabric_endpoint_cli::{print_policy, print_sessions, print_status, DaemonClient};

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
    }
    Ok(())
}
