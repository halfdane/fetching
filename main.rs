use std::env;

use clap::{Parser, Subcommand};
use tokio::sync::{mpsc, broadcast};
use spotify_player_core::{spawn_task_processor, queue_uri_tasks, Task};
use server_lib::server::setup_and_run_server;
use spotify_player_core::input::read_uris_from_file;


use tracing::info;

use spotify_player_core::create_session;
use spotify_player_core::config::Config;
use spotify_player_core::process_uris;




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

    let old = false;
    if old {
        println!("Running obsolete CLI...");
        let args: Vec<String> = env::args().collect();
        run_cli_obsolete(args).await?;
    } else {
        println!("Running new CLI...");
        run_cli_new().await?;
    }

    Ok(())
}


async fn run_cli_new() -> anyhow::Result<()> {
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

            queue_uri_tasks(urls, task_tx.clone()).await?;

            drop(task_tx);  // Close channel    
            let any_error = processor_handle.await.unwrap_or(true);    
            if any_error {        
                std::process::exit(1);    
            }    

        }
        Commands::Server { port } => {
            setup_and_run_server(task_tx, progress_tx).await?;
        }
    }
    Ok(())
}


async fn run_cli_obsolete(args: Vec<String>) -> anyhow::Result<()> {
    println!("Arguments: {:?}", args);

    let input_source = match validate_args(&args) {
        Ok(result) => result,
        Err(_) => print_usage_and_exit(&args),
    };

    let (tx, _rx) = broadcast::channel(100);

    let mut any_error = false;
    match input_source {
        InputSource::SingleUri(uri_arg) => {
            let uris = vec![uri_arg];
            if let Err(e) = process_uris(&uris, tx.clone()).await {
                eprintln!("Error: {e}");
                any_error = true;
            }
        }
        InputSource::File(path) => {
            match read_uris_from_file(&path) {
                Ok(uris) => {
                    info!("Loaded {} URIs from file", uris.len());
                    if let Err(e) = process_uris(&uris, tx.clone()).await {
                        eprintln!("Error: {e}");
                        any_error = true;
                    }
                }
                Err(e) => {
                    eprintln!("Failed to read URIs from file: {e}");
                    any_error = true;
                }
            }
        }
    }
    if any_error {
        std::process::exit(1);
    }
    Ok(())
}



#[derive(Debug)]
pub enum InputSource {
    SingleUri(String),
    File(std::path::PathBuf),
}

/// Validate command line arguments
pub fn validate_args(args: &[String]) -> anyhow::Result<InputSource> {
    let mut input_source = None;
    let mut iter = args.iter().skip(1).peekable();

    while let Some(arg) = iter.next() {
        if arg == "--file" {
            if input_source.is_some() {
                anyhow::bail!("Cannot specify both --file and a URI");
            }
            let path = iter.next()
                .ok_or_else(|| anyhow::anyhow!("--file requires a path argument"))?;
            input_source = Some(InputSource::File(std::path::PathBuf::from(path)));
        } else if input_source.is_none() {
            input_source = Some(InputSource::SingleUri(arg.clone()));
        } else {
            anyhow::bail!("Unexpected argument: {}", arg);
        }
    }

    match input_source {
        Some(source) => Ok(source),
        None => anyhow::bail!("Expected either a Spotify URI or --file <path>"),
    }
}

/// Print usage information and exit
pub fn print_usage_and_exit(args: &[String]) -> ! {
    eprintln!("Usage: {} <spotify_uri>", args[0]);
    eprintln!("       {} --file <path>", args[0]);
    eprintln!("Options:");
    eprintln!("  --file       Read URIs from file (one per line, # for comments)");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {} spotify:track:4uLU6hMCjMI75M1A2tKUQC", args[0]);
    eprintln!("  {} spotify:album:1A2GTWGtFfWp7KSQTwWOyo", args[0]);
    eprintln!("  {} spotify:playlist:37i9dQZF1DX0XUsuxWHRQd", args[0]);
    eprintln!("  {} 4uLU6hMCjMI75M1A2tKUQC  (assumes track)", args[0]);
    eprintln!("  {} --file my_uris.txt", args[0]);
    std::process::exit(1);
}


