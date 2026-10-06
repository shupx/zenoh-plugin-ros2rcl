# zenoh-plugin-ros2rcl

ROS 2 dynamic-message bridge for **Zenoh 1.10.1**, available as a plugin or standalone executable. Forward explicitly configured topics and services across machines and ROS domains.

| Configuration | Local ROS 2 | Zenoh |
| --- | --- | --- |
| `publish` | Dynamic subscription | Publisher |
| `subscribe` | Dynamic publisher | Subscriber |
| `expose_services` | Dynamic client | Queryable and reply |
| `query_services` | Dynamic service | Query and reply |

## Build

Tested on Linux with ROS 2 Humble and Rust 1.97.1. Requires a C compiler, a sourced ROS environment, and Rust ROS interfaces compatible with rclrs 0.8 / rosidl_runtime_rs 0.7, including `rcl_interfaces`.

Zenoh dependencies are pinned to 1.10.1 and fetched from crates.io. Both endpoints need matching ROS interface definitions and C/introspection type support; bridged types need no generated Rust code.

```bash
source /opt/ros/humble/setup.bash
cargo build --locked
```

Outputs in `target/debug/`: `libzenoh_plugin_ros2rcl.so` and `zenoh-bridge-ros2rcl`. For release builds, add `--release` and adjust the plugin path.

## Run

Standalone, on hosts A and B respectively:

```bash
./target/debug/zenoh-bridge-ros2rcl config/host-a.json5 config/zenoh-a.json5
./target/debug/zenoh-bridge-ros2rcl config/host-b.json5 config/zenoh-b.json5
```

Set A's reachable address in `config/zenoh-b.json5`. To load the plugin into a separately installed, ABI-compatible Zenoh 1.10.1 host:

```bash
zenohd -c config/zenohd.json5
```

## Configuration

See [host-a.json5](config/host-a.json5) and [host-b.json5](config/host-b.json5).

- `domain_id` defaults to `ROS_DOMAIN_ID`, or 0. ROS applications and configuration clients must use the corresponding domain.
- ROS names must be absolute; types use `package/msg/Type` or `package/srv/Type`.
- Export keys join `key_prefix` and the ROS name: `host_a` + `/camera/image` becomes `host_a/camera/image`. An empty prefix produces `camera/image`.
- Import routes specify concrete `key` values and local topic/service names. No prefix is added.
- Topic QoS defaults to reliable, volatile, keep-last 10. Configure `reliable`, `transient_local`, and `depth`; transient-local applies locally, without Zenoh history replay.
- `max_frequency` is optional; null means unlimited. Throttling uses a monotonic clock and `(sent + 1) / elapsed <= limit`, resetting after the decision. The window is 1 second, or `1/limit` below 1 Hz. The first message waits for budget; bursts are possible.
- `timeout_ms` defaults to 5000. Set proxy timeouts longer than exposed-service timeouts.
- `max_in_flight` defaults to 64 per service route and sets bridge DDS queue depth. Business clients/services also need sufficient queue depth.
- Invalid fields, duplicate outputs, and local feedback loops are rejected. Avoid forwarding imported topics back across the network.

## Runtime Updates

`/zenoh_ros2rcl/set_config` uses `rcl_interfaces/srv/SetParametersAtomically`. Send exactly one STRING parameter named `config`, containing the **complete configuration as JSON**:

```bash
ROS_DOMAIN_ID=10 /usr/bin/python3 scripts/set_config.py new-config.json
```

Failed updates preserve current routes. Successful updates replace all routes; omitted lists become empty. Prefixes, mappings, throttle, QoS, timeouts, and concurrency can change. `domain_id`, `node_name`, and `config_service` require restart.

Updates are held in memory and may interrupt messages or pending requests. They do not rewrite startup files or Zenoh's stored configuration. Zenoh plugin configuration updates are also supported.

## Protocol

Payload: `R2R` + version byte 1 + little-endian u32 type-name length + UTF-8 type name + ROS CDR. Receivers check version and type. This protocol is not compatible with zenoh-plugin-ros2dds.

Dynamic services use an RCL adapter with rclrs-owned messages; see [vendor/README.md](vendor/README.md). Requests are correlated individually; late replies are discarded. Queries use BestMatching. Remote errors are logged, and local ROS callers must enforce their own timeout because ROS services have no generic error response.

## Tests

```bash
cargo test --locked
/usr/bin/python3 tests/integration.py
# Optional: test a separately supplied compatible host.
ZENOH_D=/path/to/zenohd /usr/bin/python3 tests/integration.py
```

Tests use two ROS domains, defaulting to 171/172; override with `TEST_DOMAIN_A` and `TEST_DOMAIN_B`. See [TESTING.md](TESTING.md) for results.
