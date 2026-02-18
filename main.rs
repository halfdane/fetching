use clap::{Parser, Subcommand};
use tokio::sync::{mpsc, broadcast};
use fetching_core::{spawn_task_processor, queue_uri_tasks, Task};
use server_lib::server::setup_and_run_server;

use fetching_core::create_session;


#[derive(Parser)]
#[command(name = "fetching", version, about = "Spotify Player CLI", author)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Process Spotify URLs (batch mode)")]
    Batch {
        #[arg(short = 'c', default_value = "credentials.json", long = "credentials-file", help = "Path to the credentials file")] 
        credentials_file: String,
        /// Spotify track/album/playlist URLs (or @file.txt)
        #[arg(required = true, help = "One or more Spotify URLs")]
        urls: Vec<String>,
    },
    #[command(about = "Start web server + download queue")]
    Server {
        #[arg(short = 'c', default_value = "credentials.json", long = "credentials-file", help = "Path to the credentials file")] 
        credentials_file: String,
        /// Port to listen on
        #[arg(short, long, default_value_t = 8080, help = "Server port (overrides config)")]
        port: u16,
    },
}

#[tokio::main(flavor = "multi_thread", worker_threads = 3)]
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

    match cli.command {
        Commands::Batch { urls, credentials_file } => {
            let (session, _refresher, _refresh_handle) = create_session(&credentials_file).await?;
            let (progress_tx, _progress_rx) = broadcast::channel(100);
            let (task_tx, task_rx) = mpsc::channel::<Task>(100);
            let processor_handle = spawn_task_processor(&session, task_rx, progress_tx.clone());

            queue_uri_tasks(urls, task_tx.clone()).await?;
            drop(task_tx);
            let any_error = processor_handle.await.unwrap_or(true);
            if any_error {
                std::process::exit(1);
            }
        }
        Commands::Server { port, credentials_file } => {
            let (session, _refresher, _refresh_handle) = create_session(&credentials_file).await?;
            let (progress_tx, _progress_rx) = broadcast::channel(100);
            let (task_tx, task_rx) = mpsc::channel::<Task>(100);
            let processor_handle = spawn_task_processor(&session, task_rx, progress_tx.clone());

            // Pass token_path to server setup if you want to support it there too
            setup_and_run_server(task_tx, progress_tx, port).await?;
        }
    }

    Ok(())
}

