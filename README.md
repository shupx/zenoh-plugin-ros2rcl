# zenoh-plugin-ros2rcl

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/shupx/zenoh-plugin-ros2rcl)

Forward ROS 2 topics and services between machines and ROS domains using Zenoh.

## Download

Download a ZIP file from [Releases](https://github.com/shupx/zenoh-plugin-ros2rcl/releases).

The `version` should match the zenohd version. The `ros2` suffix should match your ROS distribution. The archive contains a standalone bridge and a plugin for an existing zenohd.

| Use | Archive prefix |
| --- | --- |
| Standalone bridge | `zenoh-bridge-ros2rcl-<version>-<target>-<ros2-version>` |
| Plugin for an existing zenohd | `zenoh-plugin-ros2rcl-<version>-<target>-<ros2-version>` |

## Standalone Bridge

Edit `plugins.ros2rcl` in [host-a.json5](config/host-a.json5) and [host-b.json5](config/host-b.json5) for your topics, services, and ROS domains. In B's `connect.endpoints`, replace `127.0.0.1` with A's reachable address. Allow TCP port 7447 on A.

On host A:

```bash
./zenoh-bridge-ros2rcl -c config/host-a.json5
```

On host B:

```bash
./zenoh-bridge-ros2rcl -c config/host-b.json5
```

The examples use domains 10 and 20. Run local ROS applications in the matching domain, for example `export ROS_DOMAIN_ID=10` on A.

## zenohd Plugin

For zenohd, put `libzenoh_plugin_ros2rcl.so` in the same directory as the zenohd executable, or set `plugins.ros2rcl.__path__` to the absolute path of the extracted `libzenoh_plugin_ros2rcl.so`. See [zenohd.json5](config/zenohd.json5). 

This is useful when you need to run multiple plugins in the same zenohd process.

```bash
source /opt/ros/<distro>/setup.bash
zenohd -c config/zenohd.json5
```

## Configuration

| Setting | Purpose |
| --- | --- |
| `publish` | Forward local topics to Zenoh |
| `subscribe` | Forward specified Zenoh keys to local topics |
| `expose_services` | Make local services available through Zenoh |
| `query_services` | Provide local services backed by remote Zenoh keys |
| `key_prefix` | Prefix for exported topic/service keys |
| `domain_id` | Local ROS domain; defaults to `ROS_DOMAIN_ID`, or 0 |

- ROS names must start with `/`; set `ros_type` to `package/msg/Type` or `package/srv/Type`.
- `key_prefix: "host_a"` and `/chatter` produce `host_a/chatter`. Import `key` values are used as written.
- Import routes can use different local topic/service names.
- `max_frequency` limits topic forwarding in Hz. Omit it or set `null` for unlimited forwarding; 0 is invalid.
- Topic `qos` defaults to reliable, volatile, depth 10. Set `reliable`, `transient_local`, or `depth` to match your applications. Zenoh history replay is not provided.
- Service `timeout_ms` defaults to 5000; use a longer timeout on the requesting side.
- `max_in_flight` defaults to 64 per service route. Business clients/services also need adequate DDS queue depth.
- Avoid routing imported topics back to their source. ROS callers should set a timeout for remote service failures.
- Both ends must use this bridge; its protocol differs from zenoh-plugin-ros2dds.

## Change Configuration

Send the complete `plugins.ros2rcl` configuration object (without the Zenoh wrapper) through the local service:

```bash
ROS_DOMAIN_ID=10 /usr/bin/python3 scripts/set_config.py new-config.json
```

The default service is `/zenoh_ros2rcl/set_config`; specify another with `--service`. It uses `rcl_interfaces/srv/SetParametersAtomically` with one STRING parameter named `config`.

Failed updates preserve existing routes. Successful updates replace all routes; omitted lists become empty. Changes may interrupt messages and pending calls, and are not saved to the startup file. Changing `domain_id`, `node_name`, or `config_service` requires a restart.

Developer documentation: [readme_dev.md](readme_dev.md).
