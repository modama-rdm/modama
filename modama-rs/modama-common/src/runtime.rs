//! Process lifetime helpers shared by the agent and the session helper

#[cfg(unix)]
/// Wait for Ctrl-C (or SIGTERM on Unix)
pub async fn shutdown_signal() {
    use tokio::signal::unix::SignalKind;

    let terminate = async {
        match tokio::signal::unix::signal(SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                log::error!("could not register SIGTERM handler: {error}");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(error) = result {
                log::error!("could not wait for Ctrl-C: {error}");
            }
        }
        _ = terminate => {}
    }
}

#[cfg(windows)]
/// Wait for Ctrl-C (or SIGTERM on Unix)
pub async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        log::error!("could not wait for Ctrl-C: {error}");
    }
}
