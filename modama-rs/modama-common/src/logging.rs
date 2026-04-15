pub fn init_logging() {
    let env = env_logger::Env::default()
        .filter_or("MODAMA_LOG_LEVEL", "info")
        .write_style("always");
    if let Err(e) = env_logger::try_init_from_env(env) {
        eprintln!("logging already initialized or initialization failed: {e}")
    }
}
