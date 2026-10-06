# rclrs compatibility extension

`rclrs/` is the published Apache-2.0 rclrs 0.8.0 crate (unmodified files retain
their upstream notices). Upstream: https://github.com/ros2-rust/ros2_rust.

The only local changes are in `src/dynamic_message.rs`:

- `DynamicMessage::native_ptr()` and `native_mut_ptr()` expose initialized ROS
  storage for synchronous RCL/RMW calls. No Rust layout transmutation is used.
- `DynamicMessageMetadata::new_service_message()` loads request/response
  introspection metadata from the `srv` namespace; topic metadata continues to
  use `msg` regardless of its type name.

rclrs 0.8.0 has no dynamic service/client API. `native/bridge.c` supplies these
RCL operations while request and response allocation/destruction remain owned
by rclrs `DynamicMessage`. Every native service endpoint is used only on the
bridge worker thread. Topic serialization and deserialization may run on
different threads with independent message storage and immutable type support.
