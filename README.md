# zenoh-plugin-ros2rcl

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/shupx/zenoh-plugin-ros2rcl)

Forward ROS 2 topics and services between machines and ROS domains using Zenoh.

- [Why Zenoh?](#why-zenoh) 
- [Why zenoh-plugin-ros2rcl?](#why-zenoh-plugin-ros2rcl)

## Download

Download a ZIP file from [Releases](https://github.com/shupx/zenoh-plugin-ros2rcl/releases).

The `version` should match the zenohd version. The `ros2` suffix should match your ROS distribution. The archive contains a standalone bridge and a plugin for an existing zenohd.

| Use | Archive prefix |
| --- | --- |
| Standalone bridge | `zenoh-bridge-ros2rcl-<version>-<target>-<ros2-version>` |
| Plugin for an existing zenohd | `zenoh-plugin-ros2rcl-<version>-<target>-<ros2-version>` |

`zenoh-bridge-ros2rcl` is a standalone binary that runs a Zenoh node with this plugin built-in. And `zenoh-plugin-ros2rcl` is a shared library that can be loaded by an existing zenohd binary. Both can be used according to your needs: 

### 1. Standalone Bridge

An example configuration for a three-machine setup is provided in the `config` directory: 

Edit `plugins.ros2rcl` in [host-a.json5](config/host-a.json5), [host-b.json5](config/host-b.json5), and [host-c.json5](config/host-c.json5) for your topics and services. Set the local domain with the environment variable `ROS_DOMAIN_ID` (default: 0).

The connection chain is `A -> B -> C`: A connects to B on TCP 7447, and B connects to C on TCP 7448. Replace the loopback addresses in A and B with B's and C's reachable addresses, respectively. Allow the listening ports on B and C. Multicast and gossip discovery are disabled to prevent automatic A-C connections.

On host A:

```bash
source /opt/ros/<distro>/setup.bash
./zenoh-bridge-ros2rcl -c config/host-a.json5
```

On host B:

```bash
source /opt/ros/<distro>/setup.bash
./zenoh-bridge-ros2rcl -c config/host-b.json5
```

On host C:

```bash
source /opt/ros/<distro>/setup.bash
./zenoh-bridge-ros2rcl -c config/host-c.json5
```

For more details on the zenoh configuration, see [zenoh configuration document](https://zenoh.io/docs/manual/configuration/) and [zenohd default configuration](https://github.com/eclipse-zenoh/zenoh/blob/main/DEFAULT_CONFIG.json5).

### 2. zenohd Plugin

Place `libzenoh_plugin_ros2rcl.so` in the same directory as the zenohd executable and use the same host configuration. Zenohd loads the library for the `plugins.ros2rcl` entry automatically; neither `--plugin` nor `--plugin-search-dir` is needed with the default plugin search settings.

This is useful when you need to run multiple plugins in the same zenohd process.

```bash
source /opt/ros/<distro>/setup.bash
zenohd -c config/host-a.json5
```

On host B, use `config/host-b.json5`. If the library is elsewhere, optionally specify `--plugin ros2rcl:/absolute/path/libzenoh_plugin_ros2rcl.so`, or `--plugin ros2rcl --plugin-search-dir /path/to/plugins`. `--plugin` makes the plugin required and causes zenohd to exit if it cannot load or start it.

## Configuration

An example configuration for this bridge or zenohd with this plugin loaded is below. 

```json5
{
  mode: "peer",
  listen: { endpoints: ["tcp/0.0.0.0:7447"] },
  plugins: {
    ros2rcl: {
      publish: [
        {
          ros_topic: "/chatter",
          ros_type: "std_msgs/msg/String",
          zenoh_key_prefix: "host_a",
          max_frequency: 10.0,
        },
      ],
      subscribe: [
        {
          zenoh_key: "host_b/chatter",
          ros_topic: "/remote_chatter",
          ros_type: "std_msgs/msg/String",
        },
      ],
      expose_services: [
        {
          ros_service: "/add_two_ints",
          ros_type: "example_interfaces/srv/AddTwoInts",
          zenoh_key_prefix: "host_a",
          timeout_ms: 5000,
        },
      ],
      query_services: [
        {
          zenoh_key: "host_b/add_two_ints",
          ros_service: "/remote_add_two_ints",
          ros_type: "example_interfaces/srv/AddTwoInts",
          timeout_ms: 6000,
        },
      ],
    },
  },
}
```

| Setting | Purpose |
| --- | --- |
| `publish` | Forward local topics to Zenoh. Any zenoh peer connected to this zenoh instance can get the topics if it subscribes to the corresponding Zenoh keys. |
| `subscribe` | Forward specified Zenoh keys to local topics |
| `expose_services` | Make local services available through Zenoh. Any zenoh peer connected to this zenoh instance can call the services if it queries the corresponding Zenoh keys. |
| `query_services` | Provide local services backed by remote Zenoh keys |
| `publish[].zenoh_key_prefix` | Prefix for each exported topic key; default `ros2` |
| `expose_services[].zenoh_key_prefix` | Prefix for each exported service key; default `ros2` |

- Set `ros_topic` for topic routes and `ros_service` for service routes. ROS names must start with `/`; set `ros_type` to `package/msg/Type` or `package/srv/Type`.
- Each `publish` and `expose_services` entry has its own `zenoh_key_prefix`. `"host_a"` and `/chatter` produce `host_a/chatter`; an empty prefix produces `chatter`. Import routes use the exact `zenoh_key` in `subscribe` and `query_services`.
- The local domain comes only from `ROS_DOMAIN_ID` at startup; unset means 0. Invalid values are rejected. `domain_id` and the old global `key_prefix` are not accepted in configuration.
- Import routes can use different local topic/service names.
- `max_frequency` limits topic forwarding in Hz. Omit it or set `null` for unlimited forwarding; 0 is invalid.
- Topic `qos` defaults to reliable, volatile, depth 10. Set `reliable`, `transient_local`, or `depth` to match your applications. Zenoh history replay is not provided.
- Service `timeout_ms` defaults to 5000; use a longer timeout on the requesting side.
- `max_in_flight` defaults to 64 per service route. Business clients/services also need adequate DDS queue depth.
- Avoid routing imported topics back to their source. ROS callers should set a timeout for remote service failures.
- Both ends must use this bridge; its protocol differs from zenoh-plugin-ros2dds.

## Change Configuration at Runtime

Call `/zenoh_ros2rcl/set_config` with one STRING parameter named `config`. Its value is the complete `plugins.ros2rcl` object as strict JSON, without the Zenoh wrapper. For example, change host A's topic forwarding limit to 5 Hz while keeping its routes:

```bash
ros2 service call /zenoh_ros2rcl/set_config \
  rcl_interfaces/srv/SetParametersAtomically \
  'parameters:
- name: config
  value:
    type: 4
    string_value: |
      {
        "publish": [
          {"ros_topic": "/chatter", "zenoh_key_prefix": "host_a", "ros_type": "std_msgs/msg/String", "max_frequency": 5.0}
        ],
        "subscribe": [
          {"zenoh_key": "host_b/chatter", "ros_topic": "/remote_chatter", "ros_type": "std_msgs/msg/String"}
        ],
        "expose_services": [
          {"ros_service": "/add_two_ints", "zenoh_key_prefix": "host_a", "ros_type": "example_interfaces/srv/AddTwoInts", "timeout_ms": 5000}
        ],
        "query_services": [
          {"zenoh_key": "host_b/add_two_ints", "ros_service": "/remote_add_two_ints", "ros_type": "example_interfaces/srv/AddTwoInts", "timeout_ms": 6000}
        ]
      }'
```

The ZIP also includes `scripts/set_config.py` to call this service. Save the configuration object above as `new-config.json`, then run:

```bash
/usr/bin/python3 scripts/set_config.py new-config.json
```

Use `--ros-service` to select a different configuration service. Run either command in the same ROS domain as the bridge; set `ROS_DOMAIN_ID` if needed.

Failed updates preserve existing routes. Successful updates replace all routes; omitted lists become empty. Changes may interrupt messages and pending calls, and are not saved to the startup file. Changing `ROS_DOMAIN_ID`, `node_name`, or `config_service` requires a restart.

Developer documentation: [readme_dev.md](readme_dev.md).


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

## Contributing

This project is maintained by Peixuan Shu (shupeixuan@qq.com). Contributions are welcome. Please submit a pull request or open an issue for discussion.
