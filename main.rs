use std::env;

use clap::{Parser, Subcommand};
use tokio::sync::{mpsc, broadcast};
use spotify_player_core::{spawn_task_processor, queue_uri_tasks, Task};
use server_lib::server::setup_and_run_server;
use spotify_player_core::input::read_uris_from_file;


use tracing::info;

use spotify_player_core::create_session;
use spotify_player_core::config::Config;
use spotify_player_core::process_single_url;

use uuid::Uuid;




#[derive(Parser)]
#[command(name = "spotify-player", version, about = "Spotify Player CLI", author)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Process Spotify URLs (batch mode)")]
    Batch {
        /// Spotify track/album/playlist URLs (or @file.txt)
        #[arg(required = true, help = "One or more Spotify URLs")]
        urls: Vec<String>,
    },
    #[command(about = "Start web server + download queue")]
    Server {
        /// Port to listen on
        #[arg(short, long, default_value_t = 8080, help = "Server port (overrides config)")]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    run_cli().await?;

    Ok(())
}


async fn run_cli() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let token_path = ".spotify_access_token";
    let (session, _refresher, _refresh_handle) = spotify_player_core::create_session(token_path).await?;
    let (progress_tx, _progress_rx) = broadcast::channel(100);
    let (task_tx, task_rx) = mpsc::channel::<Task>(100);
    let processor_handle = spawn_task_processor(&session, task_rx, progress_tx.clone());

    match cli.command {
        Commands::Batch { urls } => {
            if urls.is_empty() {
                eprintln!("No URLs provided. Use: spotify-player batch <urls>...");
                std::process::exit(1);
            }

            eprintln!("URLs: {:?}", urls);

            for url in urls {
                let task_id = Uuid::new_v4();
                let task = Task {
                    task_id,
                    uri: url.clone(),
                };
                let _ = task_tx.send(task).await;
            }
            // Explicitly drop the sender so the processor can exit when done
            drop(task_tx);
            processor_handle.await?;
        }
        Commands::Server { port } => {
            setup_and_run_server(task_tx, progress_tx, port).await?;
        }
    }
    Ok(())
}


