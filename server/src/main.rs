use crate::setup_and_run_server;

fn create_channels_and_spawn_worker() -> (tokio::sync::mpsc::Sender<spotify_player::Task>, tokio::sync::broadcast::Sender<spotify_player::ProgressUpdate>) {
    let (task_tx, task_rx) = tokio::sync::mpsc::channel::<spotify_player::Task>(100);
    let (progress_tx, _progress_rx) = tokio::sync::broadcast::channel::<spotify_player::ProgressUpdate>(100);

    // Spawn worker
    let progress_tx_clone = progress_tx.clone();
    tokio::spawn(async move {
        let mut task_rx = task_rx;
        while let Some(task) = task_rx.recv().await {
            let (tx, mut rx) = tokio::sync::mpsc::channel(100);
            let progress_tx_clone_for_forward = progress_tx_clone.clone();
            let forward_task = tokio::spawn(async move {
                while let Some(update) = rx.recv().await {
                    let _ = progress_tx_clone_for_forward.send(update);
                }
            });
            if let Err(_e) = spotify_player::process_url(task.task_id, task.uri.clone(), tx).await {
                let error_update = spotify_player::ProgressUpdate {
                    task_id: task.task_id,
                    scope: spotify_player::ProgressScope::Global,
                    status: "error".to_string(),
                    current: 0,
                    total: 0,
                    item: "".to_string(),
                    url: Some(task.uri),
                };
                let _ = progress_tx_clone.send(error_update);
            }
            let _ = forward_task.await;
        }
    });

    (task_tx, progress_tx)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let (task_tx, progress_tx) = create_channels_and_spawn_worker();
    setup_and_run_server(task_tx, progress_tx).await?;
    Ok(())
}
