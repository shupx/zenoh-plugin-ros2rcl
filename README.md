# zenoh-plugin-ros2rcl

面向 **Zenoh 1.10.1** 的 ROS 2 动态消息桥接插件，另提供独立桥接程序。
两端可运行在不同机器、不同 `ROS_DOMAIN_ID`，仅转发明确配置的资源。

| 配置 | 本地 ROS 2 | Zenoh |
| --- | --- | --- |
| `publish` | dynamic subscription | publisher，key = prefix + ROS topic |
| `subscribe` | dynamic publisher，可改名 | subscriber，显式 key |
| `expose_services` | dynamic RCL client | queryable + reply，key = prefix + ROS service |
| `query_services` | dynamic RCL service，可改名 | query + reply，显式 key |

## 构建

要求 Linux、C 编译器、已 source 的 ROS 2 环境。本机验证环境为
Rust 1.97.1 和 ROS 2 Humble；使用该工具链构建锁定依赖（rclrs 本身最低要求 1.85）。
ROS 环境需安装 rclrs 0.8 使用的 `rosidl_runtime_rs` 0.7 对应的
`rcl_interfaces` 等 Rust 接口（位于 `share/<package>/rust`；此机器已具备）。
桥接业务类型不需要生成 Rust 代码，但两端必须安装相同定义的 C 和 C introspection
type support，并 source 自定义接口工作空间。

仓库使用旁边的 Zenoh 源码，目录布局：

```text
zenoh_proj/
  zenoh/                    # 1.10.1 source
  zenoh-plugin-ros2rcl/
```

```bash
source /opt/ros/humble/setup.bash
cargo build --workspace --locked
cargo test --locked
```

输出 `target/debug/libzenoh_plugin_ros2rcl.so`、`zenoh-bridge-ros2rcl`、`zenohd`。
`tools/zenohd` 直接构建旁边的上游 zenohd 源码，共享锁文件，保证 Rust 插件与
宿主的 Zenoh 依赖一致。部署时使用同一 Rust 工具链和依赖构建宿主与插件。
生产构建可添加 `--release`，并修改配置中的插件路径。

## 使用

两台机器分别运行（B 的 `zenoh-b.json5` 中修改 A 的地址）：

```bash
./target/debug/zenoh-bridge-ros2rcl config/host-a.json5 config/zenoh-a.json5
./target/debug/zenoh-bridge-ros2rcl config/host-b.json5 config/zenoh-b.json5
```

或在本仓库目录启动真正的 zenohd 插件：

```bash
./target/debug/zenohd -c config/zenohd.json5
```

节点 `domain_id` 显式配置，默认读取 `ROS_DOMAIN_ID`，未设置为 0。
ROS CLI、业务节点和配置客户端也必须使用各自的 `ROS_DOMAIN_ID`。
`config/host-a.json5` 和 `host-b.json5` 展示双向话题与 service 映射。
ROS 名称必须是绝对名称；类型格式为 `package/msg/Type` 或 `package/srv/Type`。
配置字段拼写错误、重复输出、本地转发环路、无效频率/key 会被拒绝。

`key_prefix: "host_a"` 和 `/camera/image` 得到 `host_a/camera/image`。
空前缀得到 `camera/image`，key 不带首尾 `/`，只允许具体 key。
`subscribe` 和 `query_services` 的 `key` 不自动加本机前缀。
QoS 默认 reliable、volatile、keep-last 10；可配置 `reliable`、`transient_local`、
`depth`。Zenoh 历史数据重放不在本插件范围内，transient_local 是本地 ROS QoS。

`max_frequency: null` 或省略表示不限频。正数使用给定算法的平均速率判断：
`(sent+1)/elapsed <= max_frequency`，丢弃超额消息，判定后重置计数窗口。
计时用单调时钟，避免 ROS 仿真时钟回跳/暂停。>=1 Hz 使用 1 秒窗口；
<1 Hz 扩展窗口为 `1/frequency` 秒，避免持续输入被原算法永久丢弃。
首条消息需等待频率预算，不是立即发送；此算法允许窗口内突发，非等间隔定时器。

## 运行期配置 service

默认 `/zenoh_ros2rcl/set_config`，类型 `rcl_interfaces/srv/SetParametersAtomically`。
请求只接受一个 STRING 参数，名字 `config`，值为完整插件配置的 **JSON** 字符串。
返回 `result.successful` 和 `reason`；不是 ROS 标准参数服务的隐式副作用。

```bash
ROS_DOMAIN_ID=10 /usr/bin/python3 scripts/set_config.py new-config.json
```

新配置先验证并建立全部资源，失败时销毁暂存资源并继续使用原配置；成功时切换
转发资源。可以更改前缀、所有四类映射、频率、QoS、超时和并发上限。
`domain_id`、`node_name`、`config_service` 要重启，运行期修改会被拒绝。
替换采用完整配置，省略的转发列表变为空；建议保留完整启动配置再编辑。
配置替换会终止原路由的在途请求，不保证切换期间消息无损。
还支持 zenohd 的插件配置更新检查器。通过 ROS service 修改的配置仅保存在内存，
不写入启动文件或 zenohd 管理配置；后续管理配置更新以其提交的完整配置为准。

## 协议和边界

负载为 `R2R` + 版本字节 `1` + little-endian u32 类型名称长度 + UTF-8 类型名称
+ ROS CDR 数据。接收端校验类型与协议版本。两个插件必须使用相同接口定义和此协议；
不直接兼容 zenoh-plugin-ros2dds 的 key/负载协议。业务话题无需硬编码类型。
动态 service 使用小型 RCL 适配，因为上游 rclrs 尚无 dynamic service API；
请求/响应仍由 rclrs DynamicMessage 创建和销毁，见 `vendor/README.md`。

`max_in_flight` 默认每条 service 路由最多 64 个在途请求，queryable 接收队列同样有界。
本地桥接 service/client 的 DDS 队列深度采用该值；业务 service 和调用者也需设置
足够的 QoS 深度，默认深度 10 的业务端可能丢弃超过队列容量的突发请求。
请求使用 RCL sequence number/独立 request header 关联，支持并发；超时释放状态，
迟到回复丢弃。`timeout_ms` 默认 5000，建议代理超时大于服务暴露端超时。
多个相同 key 的 queryable 使用 BestMatching 选择，不做服务广播聚合。
ROS service 没有通用异常回复：远端错误/超时通过 Zenoh reply error 返回，代理记录日志，
本地 ROS 调用者需自行设置超时。配置删除也可能导致调用者超时。
避免在两端将导入话题重新导出形成网络反馈环；插件检查本机同名环路。

## 测试

```bash
cargo test --locked
/usr/bin/python3 tests/integration.py
ZENOH_D="$PWD/target/debug/zenohd" /usr/bin/python3 tests/integration.py
```

集成测试启动两个进程、两个独立 ROS domain（默认 171/172）和真实 ROS 节点，
验证双向话题、10 Hz throttle、嵌套序列消息、双向并发 service、非法配置回滚、
运行期前缀/名称/service 映射修改、资源删除和远端超时后的恢复。
可用 `TEST_DOMAIN_A`、`TEST_DOMAIN_B` 选择空闲测试 domain。测试结束清理子进程。
