pub fn init_logging() {
    let env = env_logger::Env::default()
        .filter_or("MODAMA_LOG_LEVEL", "info")
        .write_style("always");
    env_logger::init_from_env(env);
}


fn main() {
    println!("Hello, world!");
}
