use anyhow::{ensure, Result};
use zenoh::internal::{plugins::PluginsManager, runtime::RuntimeBuilder};
use zenoh_plugin_ros2rcl::Ros2RclPlugin;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 3 && matches!(args[1].as_str(), "-c" | "--config"),
        "usage: zenoh-bridge-ros2rcl -c CONFIG.json5"
    );
    let mut config = zenoh::Config::from_file(&args[2]).map_err(|e| anyhow::anyhow!("{e}"))?;
    ensure!(
        config.plugin("ros2rcl").is_some(),
        "missing plugins.ros2rcl configuration"
    );
    config
        .plugins_loading
        .set_enabled(true)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut plugins = PluginsManager::static_plugins_only();
    plugins.declare_static_plugin::<Ros2RclPlugin, &str>("ros2rcl", true);
    let mut runtime = RuntimeBuilder::new(config)
        .plugins_manager(plugins)
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    runtime.start().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    tokio::signal::ctrl_c().await?;
    runtime.close().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}
