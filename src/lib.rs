mod bridge;
pub mod config;
mod native;
mod throttle;
pub use bridge::Bridge;
use zenoh::{
    internal::{
        plugins::{RunningPluginTrait, ZenohPlugin},
        runtime::DynamicRuntime,
    },
    Wait,
};
use zenoh_plugin_trait::{plugin_long_version, plugin_version, Plugin, PluginControl};
use zenoh_util::ffi::JsonKeyValueMap;

pub struct Ros2RclPlugin;
#[cfg(feature = "dynamic_plugin")]
zenoh_plugin_trait::declare_plugin!(Ros2RclPlugin);
impl ZenohPlugin for Ros2RclPlugin {}
fn plugin_config(mut value: serde_json::Value) -> anyhow::Result<config::Config> {
    if let Some(map) = value.as_object_mut() {
        map.retain(|key, _| !key.starts_with("__"));
    }
    let config: config::Config = serde_json::from_value(value)?;
    config.validate()?;
    Ok(config)
}
impl Plugin for Ros2RclPlugin {
    type StartArgs = DynamicRuntime;
    type Instance = zenoh::internal::plugins::RunningPlugin;
    const DEFAULT_NAME: &'static str = "ros2rcl";
    const PLUGIN_VERSION: &'static str = plugin_version!();
    const PLUGIN_LONG_VERSION: &'static str = plugin_long_version!();
    fn start(name: &str, runtime: &DynamicRuntime) -> zenoh::Result<Self::Instance> {
        let value = runtime.get_config().get_plugin_config(name)?;
        let config = plugin_config(value)?;
        let session = zenoh::session::init(runtime.clone()).wait()?;
        Ok(Box::new(Running {
            bridge: Bridge::start(session, config)?,
        }))
    }
}
struct Running {
    bridge: Bridge,
}
impl PluginControl for Running {}
impl RunningPluginTrait for Running {
    fn config_checker(
        &self,
        _path: &str,
        _old: &JsonKeyValueMap,
        new: &JsonKeyValueMap,
    ) -> zenoh::Result<Option<JsonKeyValueMap>> {
        let map: serde_json::Map<String, serde_json::Value> = new.into();
        self.bridge.reconfigure(plugin_config(map.into())?)?;
        Ok(None)
    }
}
