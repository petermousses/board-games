use std::{env, net::SocketAddr};

use clap::{Parser, Subcommand};
use tracing::info;

use board_games::{api, store::Store};

#[derive(Debug, Parser)]
#[command(name = "board-games-api", about = "Authoritative game-session API")]
struct Arguments {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Copy, Subcommand)]
enum Command {
    /// Apply embedded PostgreSQL migrations and exit.
    Migrate,
    /// Run the HTTP API (the default command).
    Serve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupAction {
    Migrate,
    Serve,
}

impl Command {
    fn startup_action(self) -> StartupAction {
        match self {
            Self::Migrate => StartupAction::Migrate,
            Self::Serve => StartupAction::Serve,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let arguments = Arguments::parse();
    let database_url = env::var("DATABASE_URL").map_err(|_| "DATABASE_URL must be set")?;
    let store = Store::connect(&database_url).await?;

    match arguments.command.unwrap_or(Command::Serve).startup_action() {
        StartupAction::Migrate => {
            store.migrate().await?;
            info!("database migrations completed");
            Ok(())
        }
        StartupAction::Serve => {
            let bind_address = env::var("BIND_ADDRESS")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
                .parse::<SocketAddr>()?;
            let listener = tokio::net::TcpListener::bind(bind_address).await?;
            info!(%bind_address, "board-games API listening");
            axum::serve(listener, api::router(store))
                .with_graceful_shutdown(shutdown_signal())
                .await?;
            Ok(())
        }
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::{Command, StartupAction};

    #[test]
    fn only_migrate_command_runs_migrations() {
        assert_eq!(Command::Migrate.startup_action(), StartupAction::Migrate);
        assert_eq!(Command::Serve.startup_action(), StartupAction::Serve);
    }
}
