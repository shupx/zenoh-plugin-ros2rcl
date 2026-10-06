# Test Record

Verified on 2026-10-06 (Asia/Shanghai): Linux, ROS 2 Humble, Rust 1.97.1, rclrs 0.8.0, Zenoh 1.10.1.

Source revisions:
- Zenoh: `1211779c3647f5a96713dade452c546a07823580`
- Vendored rclrs: `c36e7a3040c3a2e299521591747b23b7e8b62a18`

Dependencies are pinned in `Cargo.lock`.

| Check | Result |
| --- | --- |
| `cargo build --workspace --locked` | Passed |
| `cargo test --locked` | 7 tests passed |
| `cargo clippy --all-targets --no-deps --locked -- -D warnings` | Passed; upstream dependency warnings remain |
| `cargo fmt -p zenoh-plugin-ros2rcl -- --check` | Passed |
| `/usr/bin/python3 tests/integration.py` | 5 tests passed, 11.470 s |
| `ZENOH_D="$PWD/target/debug/zenohd" /usr/bin/python3 tests/integration.py` | 5 tests passed, 11.203 s |

Unit tests cover configuration validation, feedback loops, throttle boundaries, sub-Hz input, dynamic topic/service CDR round trips, and invalid payloads.

Both integration runs used real ROS nodes in domains 171/172 over TCP on one machine. The plugin run loaded the shared library into two zenohd processes. Coverage:

1. Bidirectional topics, 10 Hz throttle, nested sequences, and domain isolation.
2. 24 concurrent service requests with distinct expected replies.
3. Failed-update rollback and rejection of runtime domain changes.
4. Live prefix, name, QoS, throttle, concurrency, and service mapping changes; route removal and restoration.
5. Remote timeout handling and continued operation of other services.

Bridge DDS queue depth follows `max_in_flight`; test clients/services use depth 64. Business nodes need adequate depth for bursts. All test processes exited. Deployment across two physical machines was not tested.
