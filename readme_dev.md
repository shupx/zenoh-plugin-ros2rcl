# Development

## Build

Tested on Linux with ROS 2 Humble and Rust 1.97.1. Requires a C compiler and ROS Rust interfaces compatible with rclrs 0.8 / rosidl_runtime_rs 0.7, including `rcl_interfaces`. Zenoh dependencies are pinned to 1.10.1 on crates.io.

```bash
source /opt/ros/humble/setup.bash
cargo build --locked
```

Outputs: `target/debug/libzenoh_plugin_ros2rcl.so` and `target/debug/zenoh-bridge-ros2rcl`. Add `--release` for optimized binaries. Plugin hosts must have a compatible Rust ABI.

To generate the required Rust interfaces:

```bash
bash scripts/ci-ros-interfaces.sh /tmp/ros2rcl-interfaces
source /tmp/ros2rcl-interfaces/install/setup.bash
```

This requires Git, CMake, colcon, and ROS interface generators. Business message types use runtime C/introspection type support and need no generated Rust code.

## Implementation

| Route | ROS 2 | Zenoh |
| --- | --- | --- |
| `publish` | Dynamic subscription | Publisher |
| `subscribe` | Dynamic publisher | Subscriber |
| `expose_services` | Dynamic client | Queryable and reply |
| `query_services` | Dynamic service | Query and reply |

Dynamic services use an RCL adapter with rclrs-owned messages; see [vendor/README.md](vendor/README.md). Requests are correlated individually; late replies are discarded. Queries use BestMatching. Remote errors are logged because ROS services have no generic error response.

Payload: `R2R` + version byte 1 + little-endian u32 type-name length + UTF-8 type name + ROS CDR. Receivers check version and type. This protocol differs from zenoh-plugin-ros2dds.

Throttling uses a monotonic clock and `(sent + 1) / elapsed <= limit`, resetting after the decision. The window is 1 second, or `1/limit` below 1 Hz. The first message waits for budget; bursts are possible.

Configuration updates stage resources before replacing routes. ROS service updates are held in memory; Zenoh plugin configuration updates are also supported. Neither path guarantees lossless switching.

## Tests

```bash
cargo test --locked
cargo clippy --all-targets --no-deps --locked -- -D warnings
cargo fmt -- --check
/usr/bin/python3 tests/integration.py
# Optional external-host test.
ZENOH_D=/path/to/zenohd /usr/bin/python3 tests/integration.py
```

Integration tests use ROS domains 171/172; override with `TEST_DOMAIN_A` and `TEST_DOMAIN_B`. Set `ROS2RCL_BUILD_PROFILE=release` to test release binaries. `CARGO_TARGET_DIR` is supported. Results: [TESTING.md](TESTING.md).

## Releases

[release.yml](https://github.com/shupx/zenoh-plugin-ros2rcl/blob/main/.github/workflows/release.yml) uses native amd64/arm64 runners with Ubuntu 22.04 / ROS 2 Humble containers and Rust 1.97.1. It generates ROS Rust interfaces, runs tests, builds release binaries, and packages separate plugin/bridge ZIPs with SHA-256 checksums and build metadata.

Run **Release binaries** manually from GitHub Actions. After both architectures pass, it uploads workflow artifacts and creates a draft release tagged `v<Cargo version>` at the selected commit. If run on a tag, that tag must match the Cargo version and is reused. Review the draft before publishing.

Package a local release build:

```bash
python3 scripts/package-release.py --version 0.1.0 --target x86_64-unknown-linux-gnu
```

Use `aarch64-unknown-linux-gnu` for arm64. The script checks ELF architecture and linked libraries; `--build-dir` and `--output` override defaults. Outputs go to `dist/`.
