# Development

## Build

Build separately for ROS 2 Humble (Ubuntu 22.04), Jazzy (Ubuntu 24.04), or Lyrical (Ubuntu 26.04), using Rust 1.97.1. Requires a C compiler and ROS Rust interfaces compatible with rclrs 0.8 / rosidl_runtime_rs 0.7, including `rcl_interfaces`. Zenoh dependencies are pinned to 1.10.1 on crates.io.

```bash
source /opt/ros/<distro>/setup.bash
cargo build --locked
```

Replace `<distro>` with `humble`, `jazzy`, or `lyrical`. Outputs: `target/debug/libzenoh_plugin_ros2rcl.so` and `target/debug/zenoh-bridge-ros2rcl`. Add `--release` for optimized binaries. Plugin hosts must have a compatible Rust ABI. Use a separate `CARGO_TARGET_DIR` for each distribution.

To generate the required Rust interfaces:

```bash
bash scripts/ci-ros-interfaces.sh /tmp/ros2rcl-interfaces
source /tmp/ros2rcl-interfaces/install/setup.bash
```

The script uses the sourced `ROS_DISTRO`; an optional second argument selects it explicitly. Use a separate interface workspace per distribution. This requires Git, CMake, colcon (including `python3-colcon-override-check`), and ROS interface generators. Business message types use runtime C/introspection type support and need no generated Rust code.

## Implementation

The bridge accepts `-c CONFIG.json5` (or `--config`) and statically registers the same plugin used by zenohd. Both read routes from `plugins.ros2rcl`; the previous two-file CLI is no longer supported. Runtime ROS configuration updates still take the route configuration object without the Zenoh wrapper.

| Route | ROS 2 | Zenoh |
| --- | --- | --- |
| `publish` | RCL serialized subscription | Publisher |
| `subscribe` | RCL serialized publisher | Subscriber |
| `expose_services` | Dynamic client | Queryable and reply |
| `query_services` | Dynamic service | Query and reply |

Dynamic services use an RCL adapter with rclrs-owned messages; see [vendor/README.md](vendor/README.md). Requests are correlated individually; late replies are discarded. Queries use BestMatching. Remote errors are logged because ROS services have no generic error response.

Payload: `R2R` + version byte 1 + little-endian u32 type-name length + UTF-8 type name + ROS CDR. Receivers check version and type. This protocol differs from zenoh-plugin-ros2dds.

Topics use `rcl_take_serialized_message` and `rcl_publish_serialized_message`: the plugin forwards CDR without decoding fields or re-serializing messages. Type support is still loaded at runtime. Exports reuse a receive buffer, apply throttling, then copy CDR once into the final payload. Imports retain `ZBytes` and borrow contiguous CDR for publishing; fragmented payloads may require coalescing. RMW/transport layers may still copy data, and application ROS publishers/subscribers still serialize/deserialize.

Native topic endpoints stay on the ROS worker, retaining the existing topic/service node names. Zenoh callbacks enqueue payload references; the worker processes up to 64 messages per topic per pass and sleeps 1 ms between passes. The import queue is unbounded to avoid dropping accepted messages; sustained overload can grow memory. Receive buffers retain their largest capacity until route cleanup. Header/version/type checks remain; deeper CDR validation is delegated to RMW and ROS consumers.

Services retain dynamic messages. Each service codec reuses a mutex-protected RMW serialization buffer and copies CDR once into the final payload. Service replies retain `ZBytes` across threads.

Throttling uses a monotonic clock and `(sent + 1) / elapsed <= limit`, resetting after the decision. The window is 1 second, or `1/limit` below 1 Hz. The first message waits for budget; bursts are possible.

Configuration updates stage resources before replacing routes. ROS service updates are held in memory; Zenoh plugin configuration updates are also supported. Neither path guarantees lossless switching.

The domain is read once from `ROS_DOMAIN_ID` (unset: 0). Route fields are `ros_topic`, `ros_service`, and `zenoh_key`. Export routes each have a `zenoh_key_prefix` (default: `ros2`); there is no global prefix or configurable domain.

## Tests

```bash
cargo test --locked
cargo clippy --all-targets --no-deps --locked -- -D warnings
cargo fmt -- --check
cargo test --release --locked large_message_encode_benchmark -- --ignored --nocapture
/usr/bin/python3 tests/integration.py
# Optional external-host test.
ZENOH_D=/path/to/zenohd /usr/bin/python3 tests/integration.py
```

Integration tests use ROS domains 171/172; override with `TEST_DOMAIN_A` and `TEST_DOMAIN_B`. The serialized compatibility unit test uses domain 173 (`TEST_DOMAIN_NATIVE` overrides it). Set `ROS2RCL_BUILD_PROFILE=release` to test release binaries. `CARGO_TARGET_DIR` is supported. Results: [TESTING.md](TESTING.md).

## Releases

[release.yml](https://github.com/shupx/zenoh-plugin-ros2rcl/blob/main/.github/workflows/release.yml) uses native amd64/arm64 runners with Humble/Jazzy/Lyrical containers and Rust 1.97.1. All six builds generate ROS Rust interfaces, run unit and integration tests, and package separate plugin/bridge ZIPs with SHA-256 checksums and build metadata. Archive and artifact names include the ROS distribution.

Run **Release binaries** manually from GitHub Actions. After all builds pass, it creates a draft release tagged `v<Cargo version>` at the selected commit. If run on a tag, that tag must match the Cargo version and is reused. Review the draft before publishing.

Package a local release build:

```bash
python3 scripts/package-release.py --version 1.10.1 --target x86_64-unknown-linux-gnu --ros-distro "$ROS_DISTRO"
```

Use `aarch64-unknown-linux-gnu` for arm64. The script checks the sourced ROS distribution, ELF architecture, and linked libraries; `--build-dir` and `--output` override defaults. Outputs go to `dist/`.
