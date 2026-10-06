use anyhow::{ensure, Result};
use zenoh::Wait;
use zenoh_plugin_ros2rcl::{config::Config, Bridge};
fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 2 || args.len() == 3,
        "usage: zenoh-bridge-ros2rcl ROS_CONFIG.json [ZENOH_CONFIG.json5]"
    );
    let config: Config = json5::from_str(&std::fs::read_to_string(&args[1])?)?;
    let zenoh_config = if args.len() == 3 {
        zenoh::Config::from_file(&args[2]).map_err(|e| anyhow::anyhow!("{e}"))?
    } else {
        zenoh::Config::default()
    };
    let session = zenoh::open(zenoh_config)
        .wait()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let bridge = Bridge::start(session, config)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(tokio::signal::ctrl_c())?;
    drop(bridge);
    Ok(())
}
