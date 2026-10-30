/// Info by default, but zbus and its tracing spans only from warn on: they log every D-Bus
/// handshake at info. `MODAMA_LOG_LEVEL` is applied on top, so it can still turn them up
/// (`debug,zbus=debug`).
fn builder(env: env_logger::Env) -> env_logger::Builder {
    let mut builder = env_logger::Builder::new();
    builder
        .filter_level(log::LevelFilter::Info)
        .filter_module("zbus", log::LevelFilter::Warn)
        .filter_module("tracing", log::LevelFilter::Warn)
        .parse_env(env);
    builder
}

pub fn init_logging() {
    let env = env_logger::Env::default()
        .filter("MODAMA_LOG_LEVEL")
        .write_style("always");
    if let Err(e) = builder(env).try_init() {
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
        .filter("MODAMA_LOG_LEVEL")
        .write_style("never");
    builder(env)
        .target(env_logger::Target::Pipe(Box::new(writer)))
        .try_init()
        .map_err(Into::into)
}
