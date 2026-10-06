# Test Record

验证日期：2026-10-06（Asia/Shanghai）。

环境：Linux、ROS 2 Humble、Rust 1.97.1、rclrs 0.8.0、Zenoh 1.10.1。
Zenoh 源码提交：`1211779c3647f5a96713dade452c546a07823580`。
vendored rclrs 发布包源提交：`c36e7a3040c3a2e299521591747b23b7e8b62a18`。
依赖由本仓库 `Cargo.lock` 固定。

| 检查 | 结果 |
| --- | --- |
| `cargo build --workspace --locked` | 通过；生成动态插件、独立桥接程序、上游 zenohd |
| `cargo test --locked` | 7 项单元测试通过 |
| `cargo clippy --all-targets --no-deps --locked -- -D warnings` | 插件代码通过；vendored 上游代码仍有编译警告 |
| `cargo fmt -p zenoh-plugin-ros2rcl -- --check` | 通过 |
| `/usr/bin/python3 tests/integration.py` | 5 项真实集成测试通过，11.470 秒 |
| `ZENOH_D="$PWD/target/debug/zenohd" /usr/bin/python3 tests/integration.py` | 5 项真实插件集成测试通过，11.203 秒 |

集成测试在同一台机器创建两个独立的 ROS domain（171 和 172），通过 Zenoh TCP
连接。第二轮确实由两个 zenohd 进程加载 `libzenoh_plugin_ros2rcl.so` 完成转发。
未使用 mock，未对两台物理机器进行网络部署验证。测试后所有子进程已退出。

单元测试覆盖：配置与名称/key 校验、反馈环拒绝、给定 throttle 算法边界、
连续输入下的亚赫兹限频、动态话题/服务消息 CDR 往返、错误版本、截断负载和类型不匹配。

集成测试覆盖：

1. 双向 ROS topic，10 Hz throttle，包含嵌套结构和可变序列的 Float64MultiArray
   完整内容校验，同时验证原始 ROS domain 隔离。
2. 双向共 24 个并发 AddTwoInts 请求，逐个验证不同请求对应的回复。
3. 不存在的动态消息类型导致资源建立失败，旧配置和 service 继续工作；
   运行期 domain 修改被拒绝。
4. 通过 ROS config service 修改 key 前缀、话题名称、限频、QoS、并发上限、
   被暴露 service 的本地名称、代理 service 的名称与 query key；旧映射停止转发；
   删除全部路由后不再转发，再恢复配置。
5. 不存在的本地 service 导致 Zenoh reply error/超时，业务 ROS 调用者未收到
   伪造响应；其他正常 service 继续工作并可恢复配置。

并发测试曾暴露默认 DDS service 深度 10 无法容纳 12 个突发请求的问题。
插件 service/client 的深度现为 `max_in_flight`，测试业务 service/client 深度为 64。
这是 DDS 队列容量约束；部署时业务节点也应配置足够的服务 QoS 深度。
