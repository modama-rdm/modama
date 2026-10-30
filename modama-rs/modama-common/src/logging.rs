pub fn init_logging() {
    let env = env_logger::Env::default()
        .filter_or("MODAMA_LOG_LEVEL", "info")
        .write_style("always");
    if let Err(e) = env_logger::try_init_from_env(env) {
        eprintln!("logging already initialized or initialization failed: {e}")
    }
}

/// Log into rotating files `log_dir/file_name`, for processes without a console
#[cfg(windows)]
pub fn init_file_logging(log_dir: &std::path::Path, file_name: &str) -> anyhow::Result<()> {
    use file_rotate::{ContentLimit, FileRotate, compression::Compression, suffix::AppendCount};

    std::fs::create_dir_all(log_dir)?;
    let writer = FileRotate::new(
        log_dir.join(file_name),
        AppendCount::new(5),
        ContentLimit::BytesSurpassed(10 * 1024 * 1024),
        Compression::None,
        None,
    );
    let env = env_logger::Env::default()
        .filter_or("MODAMA_LOG_LEVEL", "info")
        .write_style("never");
    env_logger::Builder::from_env(env)
        .target(env_logger::Target::Pipe(Box::new(writer)))
        .try_init()
        .map_err(Into::into)
}
