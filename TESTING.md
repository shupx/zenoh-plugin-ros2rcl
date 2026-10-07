# Test Record

## Serialized Topic Forwarding

Verified on 2026-10-07: ROS 2 Humble / amd64, release profile. Topic routes now use RCL serialized take/publish; services retain dynamic messages. The wire format and configuration are unchanged.

- 10 unit tests passed, including legacy codec payload compatibility with real serialized ROS endpoints; the manual encode benchmark remains ignored.
- 6 integration tests passed with Fast DDS and 6 with CycloneDDS. Coverage includes bidirectional topics, exact 1 MiB/8 MiB strings followed by short/empty strings, nested sequences, throttling, transient-local history on export/import, dynamic QoS/route updates, rollback, concurrent services, and timeouts.
- Release build, clippy, and formatting checks passed; upstream dependency warnings remain.

This removes plugin-level topic serialization/deserialization, not application or middleware work. No end-to-end speedup factor is claimed. Topic imports queue retained payloads on the worker; sustained overload can grow the queue. Malformed headers/types/truncated CDR are rejected; deeper CDR validation is delegated to RMW/consumers. Jazzy/Lyrical and arm64 were not rerun for this change.

## Copy and Allocation Optimization

Verified on 2026-10-07: ROS 2 Humble / amd64, Rust 1.97.1, release profile. All 9 unit tests and 5 integration tests passed; release clippy and formatting checks passed. The manual benchmark is ignored in normal test runs. Concurrent codec tests alternate large/small messages across four threads and verify payload ownership and round trips. Integration tests also forward a 1 MiB string followed by a short string and verify exact contents.

Warm `std_msgs/msg/String` encoding benchmark on the same machine:

| String size | Before (µs/message) | After (µs/message) |
| --- | --- | --- |
| 1 KiB | 1.1 | 0.7 |
| 1 MiB | 1537.3 | 251.5 |
| 8 MiB | 13555.6 | 2268.6 |

These timings measure `Codec::encode`, including serialization and payload allocation, not network/ROS end-to-end latency. Buffers were warmed up; 1 KiB used 10,000 iterations, larger sizes 100. Each codec retains its maximum serialization capacity until route cleanup and serializes encode calls with a mutex. Topic APIs, wire format, type validation, QoS, throttling, concurrent services, timeouts, and configuration behavior are retained. Jazzy/Lyrical and arm64 were not rerun for this optimization.

Configuration schema update verified on 2026-10-07 with ROS 2 Humble / amd64: 8 unit tests and 5 integration tests passed. Bridge processes use `ROS_DOMAIN_ID` for domain isolation. Tests cover independent per-route topic/service prefixes, live prefix updates, legacy-field rejection, and rollback. Startup with an unset domain uses 0; invalid environment values are rejected. `set_config.py --ros-service` successfully updates the domain-0 bridge. Jazzy/Lyrical and arm64 were not rerun for this update.

Verified on 2026-10-06 (Asia/Shanghai): Linux, ROS 2 Humble, Rust 1.97.1, rclrs 0.8.0, Zenoh 1.10.1.

Zenoh 1.10.1 dependencies come from crates.io and are pinned in `Cargo.lock`.
Vendored rclrs revision: `c36e7a3040c3a2e299521591747b23b7e8b62a18`.

| Check | Result |
| --- | --- |
| `cargo build --locked` | Passed from an isolated temporary directory |
| `cargo test --locked` | 7 tests passed |
| `cargo clippy --all-targets --no-deps --locked -- -D warnings` | Passed; upstream dependency warnings remain |
| `cargo fmt -p zenoh-plugin-ros2rcl -- --check` | Passed |
| `/usr/bin/python3 tests/integration.py` | 5 tests passed, 11.439 s |
| `ZENOH_D=/path/to/zenohd /usr/bin/python3 tests/integration.py` | 5 tests passed with an existing compatible host, 11.227 s |

Unit tests cover configuration validation, feedback loops, throttle boundaries, sub-Hz input, dynamic topic/service CDR round trips, and invalid payloads.

Both integration runs used real ROS nodes in domains 171/172 over TCP on one machine. The plugin run loaded the shared library into two zenohd processes. Coverage:

1. Bidirectional topics, 10 Hz throttle, nested sequences, and domain isolation.
2. 24 concurrent service requests with distinct expected replies.
3. Failed-update rollback and rejection of runtime domain changes.
4. Live prefix, name, QoS, throttle, concurrency, and service mapping changes; route removal and restoration.
5. Remote timeout handling and continued operation of other services.

Bridge DDS queue depth follows `max_in_flight`; test clients/services use depth 64. Business nodes need adequate depth for bursts. All test processes exited. Deployment across two physical machines was not tested.

## Release Workflow

Unified configuration verified on ROS 2 Humble / amd64: release build, 7 unit tests, and 5 integration tests passed using `-c` and `plugins.ros2rcl`. Runtime service updates continue to accept the route object without the Zenoh wrapper.

Locally verified: actionlint, ROS Rust interface generation, 7 unit tests, amd64 release build, and 5 release integration tests. Both ZIP archives passed content, executable permission, and SHA-256 checks; incorrect architecture labels were rejected.

GitHub Actions and arm64 builds have not been executed locally.

## Multiple ROS Distributions

Verified on 2026-10-06 with Rust 1.97.1 and Zenoh 1.10.1:

| ROS 2 / amd64 | Interface generation | Release unit tests | Release integration tests | ZIPs / SHA-256 |
| --- | --- | --- | --- | --- |
| Humble / Ubuntu 22.04 | Passed | 7 passed | 5 passed | Passed |
| Jazzy / Ubuntu 24.04 container | Passed | 7 passed | 5 passed | Passed |
| Lyrical / Ubuntu 26.04 container | Passed | 7 passed | 5 passed | Passed |

Jazzy/Lyrical also generate `service_msgs` Rust interfaces. Unsupported interface distributions and mismatched packaging environments are rejected. Workflow syntax and Rust formatting checks passed. Official release images provide amd64 and arm64; arm64 execution remains for GitHub Actions.
