# zenoh-plugin-ros2rcl

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/shupx/zenoh-plugin-ros2rcl)

Forward ROS 2 topics and services between machines and ROS domains using Zenoh.

## Why zenoh

1. [Zenoh](https://zenoh.io/docs) provides exellent **dynamic discovery of nodes and routing of data**. The zenoh nodes can be connected manually, or automatically by [multicast scouting or through a Zenoh router](https://zenoh.io/docs/getting-started/deployment/). Therefore, you can connect multiple zenoh nodes together to form any network topology. Two zenoh peers that are not directly connected can also exchange data through a Zenoh router. 

2. One Zenoh node uses only a single TCP port (if use TCP, which is the default). All traffic between two zenoh nodes is through a single TCP port. This is useful for traversing firewalls and NATs. And zenoh also manages the connection and reconnection of TCP connections automatically.

3. Zenoh is designed to be **data-centric**. The connection topology only defines the routing of data, and the consumers and producers of data do not need to know each other. Zenoh provides a [**key-based addressing**](https://zenoh.io/docs/manual/abstractions/) for data, which is similar to the topic names in ROS 2. Therefore, Zenoh can be used as a **data-centric middleware** for ROS 2.

So I think **Zenoh is the best choice and future for multi-machine communication**, and is way much better than DDS (heavy and complex) or MQTT (need centered broker).

### How to use zenoh

Two ways:

- Use the [C/C++/Python/Rust Zenoh API](https://zenoh.io/docs/apis/rust/) to implement your own zenoh nodes (peers). And publish/subscribe data or query/reply data through Zenoh. This is the most native way to use Zenoh, but is not relevant to this plugin users.

- Use the [zenohd](https://zenoh.io/docs/getting-started/installation/#installing-the-zenoh-router), a prebuilt zenoh node binary that almost requires no extra dependencies. Load zenoh plugins on zenohd to extend its functionality. For example, the [zenoh-plugin-remote-api](https://github.com/eclipse-zenoh/zenoh-ts/tree/main/zenoh-plugin-remote-api) starts a websocket server on zenohd, and allows web clients connect to it to perform publish/subscribe and query/reply using [typescript API](https://github.com/eclipse-zenoh/zenoh-ts), which is very useful for web applications. And this plugin, `zenoh-plugin-ros2rcl`, is also a zenoh plugin that translates ROS 2 topics and services to Zenoh keys, and allows ROS 2 nodes on different machines and ROS domains to communicate with each other through Zenoh.

So the basic experience of using this plugin is to run a `zenohd` binary with this plugin `zenoh-plugin-ros2rcl` loaded on each machine, and specify the topics and services to forward in the configuration file. Then, the ROS 2 nodes on different machines can communicate with each other through Zenoh.

If you do not want to use `zenohd`, you can also use the standalone bridge `zenoh-bridge-ros2rcl` binary provided by this plugin, which is a zenoh node with this plugin built-in. 


## Why zenoh-plugin-ros2rcl

There is also an another Zenoh plugin for ROS 2, [zenoh-plugin-ros2dds](https://github.com/eclipse-zenoh/zenoh-plugin-ros2dds). Here are the main differences between the two plugins:

- This plugin is **independent of the underlying ROS 2 RMW implementation**, since it is built on [rclrs](https://github.com/ros2-rust/ros2_rust), which provides the official Rust bindings to the ROS 2 `rcl` C API. In comparison, [zenoh-plugin-ros2dds](https://github.com/eclipse-zenoh/zenoh-plugin-ros2dds) is built on top of the ROS 2 cyclonedds RMW and is not compatible with other RMW implementations.

- This plugin does not transport all ROS 2 messages and services over Zenoh. Instead, it **forwards only the topics and services that you configure**. 

- **The sending frequency of forwarded topics can be limited** to reduce network traffic. **The ROS topic names at the receiving end can be configured** to be different from the sending end. These configurations can be set by the configuration file and changed at runtime through a ROS service call. All these features are not available in [zenoh-plugin-ros2dds](https://github.com/eclipse-zenoh/zenoh-plugin-ros2dds).

There is also a ROS 2 RMW implementation for Zenoh, [rmw_zenoh](https://github.com/ros2/rmw_zenoh). It is designed to be a drop-in replacement for the existing ROS2 RMW implementations, and should not be used for multi-machine communication unless all machines share the same ROS domain. In comparison, this plugin is designed for multi-machine communication and can be used with any RMW implementation.

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
