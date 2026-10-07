# rclrs Extension

`rclrs/` vendors the Apache-2.0 [rclrs 0.8.0](https://github.com/ros2-rust/ros2_rust) crate. Upstream notices are retained.

Only `src/dynamic_message.rs` is modified:

- `native_ptr()` and `native_mut_ptr()` expose message storage for synchronous RCL/RMW calls.
- `new_service_message()` loads request/response metadata from the `srv` namespace.

Upstream lacks dynamic service/client APIs. `native/bridge.c` supplies them while rclrs owns message allocation and destruction. Topics use RCL serialized take/publish directly. Native endpoints stay on the worker thread.
