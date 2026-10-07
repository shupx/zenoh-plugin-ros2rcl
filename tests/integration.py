#!/usr/bin/env python3
"""Real ROS 2 / Zenoh tests on two isolated ROS domains; no mocks."""
import copy
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import unittest

import rclpy
from rclpy.context import Context
from rclpy.executors import SingleThreadedExecutor
from rclpy.qos import QoSProfile
from std_msgs.msg import String, Float64MultiArray, MultiArrayDimension
from example_interfaces.srv import AddTwoInts
from rcl_interfaces.msg import Parameter, ParameterValue
from rcl_interfaces.srv import SetParametersAtomically

ROOT = Path(__file__).resolve().parents[1]
BUILD_PROFILE = os.environ.get("ROS2RCL_BUILD_PROFILE", "debug")
TARGET_DIR = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class BridgeIntegration(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix="ros2rcl-test-")
        cls.logs, cls.processes, cls.nodes, cls.contexts, cls.executors = [], [], [], [], []
        cls.addClassCleanup(cls.cleanup)
        domain_a = int(os.environ.get("TEST_DOMAIN_A", "171"))
        domain_b = int(os.environ.get("TEST_DOMAIN_B", "172"))
        cls.a_config = {
            "publish": [{"ros_topic": "/source", "ros_type": "std_msgs/msg/String", "max_frequency": 10.0},
                        {"ros_topic": "/array", "ros_type": "std_msgs/msg/Float64MultiArray"}],
            "subscribe": [{"zenoh_key": "site_b/source", "ros_topic": "/from_b", "ros_type": "std_msgs/msg/String"}],
            "expose_services": [{"ros_service": "/add", "ros_type": "example_interfaces/srv/AddTwoInts", "timeout_ms": 1000}],
            "query_services": [{"zenoh_key": "site_b/add", "ros_service": "/remote_add", "ros_type": "example_interfaces/srv/AddTwoInts", "timeout_ms": 1500}],
        }
        cls.b_config = {
            "publish": [{"ros_topic": "/source", "ros_type": "std_msgs/msg/String"}],
            "subscribe": [{"zenoh_key": "site_a/source", "ros_topic": "/from_a", "ros_type": "std_msgs/msg/String"},
                          {"zenoh_key": "site_a/array", "ros_topic": "/from_array", "ros_type": "std_msgs/msg/Float64MultiArray"}],
            "expose_services": [{"ros_service": "/add", "ros_type": "example_interfaces/srv/AddTwoInts", "timeout_ms": 1000}],
            "query_services": [{"zenoh_key": "site_a/add", "ros_service": "/remote_add", "ros_type": "example_interfaces/srv/AddTwoInts", "timeout_ms": 1500}],
        }
        for cfg, prefix in [(cls.a_config, "site_a"), (cls.b_config, "site_b")]:
            for route in cfg["publish"] + cfg["expose_services"]:
                route["zenoh_key_prefix"] = prefix
        p = port()
        z_a = {"mode": "peer", "listen": {"endpoints": [f"tcp/127.0.0.1:{p}"]},
               "scouting": {"multicast": {"enabled": False}}}
        z_b = {"mode": "peer", "connect": {"endpoints": [f"tcp/127.0.0.1:{p}"]},
               "scouting": {"multicast": {"enabled": False}}}
        for i, (cfg, zcfg) in enumerate([(cls.a_config, z_a), (cls.b_config, z_b)]):
            zfile = Path(cls.temp.name) / f"zenoh{i}.json"
            zcfg["plugins"] = {"ros2rcl": dict(cfg, __required__=True)}
            log = open(Path(cls.temp.name) / f"bridge{i}.log", "w+")
            cls.logs.append(log)
            domain = [domain_a, domain_b][i]
            env = dict(os.environ, RUST_LOG="info", ROS_DOMAIN_ID=str(domain))
            if os.environ.get("ZENOH_D"):
                zcfg["plugins"]["ros2rcl"]["__path__"] = str(TARGET_DIR / BUILD_PROFILE / "libzenoh_plugin_ros2rcl.so")
                cmd = [os.environ["ZENOH_D"], "-c", str(zfile)]
            else:
                cmd = [str(TARGET_DIR / BUILD_PROFILE / "zenoh-bridge-ros2rcl"), "-c", str(zfile)]
            zfile.write_text(json.dumps(zcfg))
            cls.processes.append(subprocess.Popen(cmd, stdout=log, stderr=log, env=env))
            ctx = Context()
            rclpy.init(context=ctx, domain_id=domain)
            node = rclpy.create_node(f"integration_{i}", context=ctx)
            executor = SingleThreadedExecutor(context=ctx)
            executor.add_node(node)
            cls.contexts.append(ctx)
            cls.nodes.append(node)
            cls.executors.append(executor)
        cls.services = []
        for i, node in enumerate(cls.nodes):
            def add(req, resp, offset=i * 1000):
                resp.sum = req.a + req.b + offset
                return resp
            cls.services.append(node.create_service(AddTwoInts, "/add", add, qos_profile=QoSProfile(depth=64)))
        cls.pub_a = cls.nodes[0].create_publisher(String, "/source", 10)
        cls.pub_b = cls.nodes[1].create_publisher(String, "/source", 10)
        cls.pub_array = cls.nodes[0].create_publisher(Float64MultiArray, "/array", 10)
        cls.got_a, cls.got_b, cls.got_array, cls.unbridged = [], [], [], []
        cls.subs = [cls.nodes[0].create_subscription(String, "/from_b", lambda m: cls.got_a.append(m.data), 100),
                    cls.nodes[1].create_subscription(String, "/from_a", lambda m: cls.got_b.append(m.data), 100),
                    cls.nodes[1].create_subscription(Float64MultiArray, "/from_array", cls.got_array.append, 10),
                    cls.nodes[1].create_subscription(String, "/source", lambda m: cls.unbridged.append(m.data), 10)]
        cls.clients = [n.create_client(AddTwoInts, "/remote_add", qos_profile=QoSProfile(depth=64)) for n in cls.nodes]
        cls.controls = [n.create_client(SetParametersAtomically, "/zenoh_ros2rcl/set_config") for n in cls.nodes]
        cls.pump(3)
        cls.check_processes()
        for client in cls.controls + cls.clients:
            if not client.wait_for_service(timeout_sec=10):
                cls.dump_logs()
                raise RuntimeError(f"service not discovered: {client.srv_name}")

    @classmethod
    def check_processes(cls):
        for process in cls.processes:
            if process.poll() is not None:
                cls.dump_logs()
                raise RuntimeError(f"bridge exited {process.returncode}")

    @classmethod
    def dump_logs(cls):
        for log in cls.logs:
            log.flush()
            log.seek(0)
            print(log.read())

    @classmethod
    def pump(cls, seconds, predicate=None):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            for executor in cls.executors:
                executor.spin_once(timeout_sec=0.002)
            if predicate and predicate():
                return True
        return predicate() if predicate else True

    def change(self, i, config, success=True):
        request = SetParametersAtomically.Request(parameters=[Parameter(name="config", value=ParameterValue(type=4, string_value=json.dumps(config)))])
        future = self.controls[i].call_async(request)
        self.assertTrue(self.pump(10, future.done), "configuration response timed out")
        self.assertEqual(future.result().result.successful, success, future.result().result.reason)

    def test_01_bidirectional_topics_and_nested_sequence(self):
        self.got_a.clear(); self.got_b.clear(); self.got_array.clear(); self.unbridged.clear()
        array = Float64MultiArray(data=[1.25, -2.5, 3.75])
        array.layout.dim = [MultiArrayDimension(label="axis", size=3, stride=3)]
        array.layout.data_offset = 2
        for i in range(50):
            self.pub_a.publish(String(data=f"a-{i}"))
            self.pub_b.publish(String(data=f"b-{i}"))
            self.pub_array.publish(array)
            self.pump(0.02)
        self.pump(0.5)
        self.assertGreater(len(self.got_a), 10)
        self.assertGreater(len(self.got_b), 2)
        self.assertLessEqual(len(self.got_b), 18, "10 Hz throttle exceeded")
        self.assertTrue(self.got_array)
        self.assertEqual(self.got_array[-1], array)
        self.assertFalse(any(x.startswith("a-") for x in self.unbridged), "ROS domains were not isolated")
        # Large payload followed by a short one exercises reusable codec capacity
        # and Zenoh transport without retaining bytes from the previous message.
        for payload in ["large:" + "x" * (1024 * 1024), "short-after-large"]:
            count = len(self.got_a)
            self.pub_b.publish(String(data=payload))
            self.assertTrue(self.pump(5, lambda: len(self.got_a) > count), "large-message delivery timed out")
            self.assertEqual(self.got_a[-1], payload)

    def test_02_concurrent_bidirectional_services(self):
        futures = []
        for i, client in enumerate(self.clients):
            for j in range(12):
                futures.append((client.call_async(AddTwoInts.Request(a=j, b=2*j)), 3*j + (1-i)*1000))
        self.assertTrue(self.pump(10, lambda: all(f.done() for f, _ in futures)))
        for future, expected in futures:
            self.assertEqual(future.result().sum, expected)

    def test_03_failed_config_preserves_routes(self):
        invalid = copy.deepcopy(self.b_config)
        invalid["subscribe"].append({"zenoh_key": "other/key", "ros_topic": "/bad", "ros_type": "nonexistent_pkg/msg/Missing"})
        self.change(1, invalid, success=False)
        future = self.clients[1].call_async(AddTwoInts.Request(a=40, b=2))
        self.assertTrue(self.pump(5, future.done))
        self.assertEqual(future.result().sum, 42)
        invalid = copy.deepcopy(self.b_config)
        invalid["domain_id"] = 42
        self.change(1, invalid, success=False)

    def test_04_live_topic_prefix_service_mapping_and_removal(self):
        a = copy.deepcopy(self.a_config); b = copy.deepcopy(self.b_config)
        a["publish"][0]["zenoh_key_prefix"] = "renamed_a"
        a["publish"][1]["zenoh_key_prefix"] = "arrays_a"
        a["expose_services"][0]["zenoh_key_prefix"] = "services_a"
        a["max_in_flight"] = 32
        b["max_in_flight"] = 32
        a["publish"][0]["max_frequency"] = None
        a["publish"][0]["qos"] = {"reliable": False, "depth": 20}
        a["subscribe"][0]["ros_topic"] = "/renamed_from_b"
        a["query_services"][0]["ros_service"] = "/renamed_remote"
        a["expose_services"][0]["ros_service"] = "/renamed_add"
        b["subscribe"][0].update(zenoh_key="renamed_a/source", ros_topic="/renamed_from_a")
        b["subscribe"][1]["zenoh_key"] = "arrays_a/array"
        b["query_services"][0]["zenoh_key"] = "services_a/renamed_add"
        def renamed_add(req, resp):
            resp.sum = req.a + req.b
            return resp
        newservice = self.nodes[0].create_service(AddTwoInts, "/renamed_add", renamed_add)
        received = [[], []]
        newsubs = [self.nodes[0].create_subscription(String, "/renamed_from_b", lambda m: received[0].append(m.data), 100),
                   self.nodes[1].create_subscription(String, "/renamed_from_a", lambda m: received[1].append(m.data), 100)]
        self.change(0, a); self.change(1, b); self.pump(2)
        array_count = len(self.got_array)
        for _ in range(5):
            self.pub_array.publish(Float64MultiArray(data=[42.0]))
            self.pump(0.05)
        self.assertGreater(len(self.got_array), array_count, "independent array prefix was not applied")
        self.assertEqual(list(self.got_array[-1].data), [42.0])
        old_a, old_b = len(self.got_a), len(self.got_b)
        for _ in range(30):
            self.pub_a.publish(String(data="new-a")); self.pub_b.publish(String(data="new-b")); self.pump(0.02)
        self.pump(0.3)
        self.assertGreater(len(received[0]), 15); self.assertGreater(len(received[1]), 15)
        self.assertEqual(len(self.got_a), old_a); self.assertEqual(len(self.got_b), old_b)
        client = self.nodes[0].create_client(AddTwoInts, "/renamed_remote")
        self.assertTrue(client.wait_for_service(timeout_sec=5))
        future = client.call_async(AddTwoInts.Request(a=3, b=4))
        self.assertTrue(self.pump(5, future.done)); self.assertEqual(future.result().sum, 1007)
        future = self.clients[1].call_async(AddTwoInts.Request(a=20, b=22))
        self.assertTrue(self.pump(5, future.done)); self.assertEqual(future.result().sum, 42)
        # Remove every forwarding resource and verify new names stop receiving.
        a.update(publish=[], subscribe=[], expose_services=[], query_services=[])
        b.update(publish=[], subscribe=[], expose_services=[], query_services=[])
        self.change(0, a); self.change(1, b); self.pump(1)
        count = [len(x) for x in received]
        for _ in range(10):
            self.pub_a.publish(String(data="removed")); self.pub_b.publish(String(data="removed")); self.pump(0.03)
        self.assertEqual([len(x) for x in received], count)
        self.pump(1)
        self.assertFalse(client.service_is_ready())
        for node, sub in zip(self.nodes, newsubs):
            node.destroy_subscription(sub)
        self.nodes[0].destroy_service(newservice)
        self.change(0, self.a_config); self.change(1, self.b_config)

    def test_05_timeout_and_recovery(self):
        b = copy.deepcopy(self.b_config)
        b["query_services"].append({"zenoh_key": "site_a/missing", "ros_service": "/missing_proxy", "ros_type": "example_interfaces/srv/AddTwoInts", "timeout_ms": 300})
        a = copy.deepcopy(self.a_config)
        a["expose_services"].append({"ros_service": "/missing", "zenoh_key_prefix": "site_a", "ros_type": "example_interfaces/srv/AddTwoInts", "timeout_ms": 100})
        self.change(0, a); self.change(1, b)
        client = self.nodes[1].create_client(AddTwoInts, "/missing_proxy")
        self.assertTrue(client.wait_for_service(timeout_sec=5))
        future = client.call_async(AddTwoInts.Request(a=1, b=2))
        self.pump(1)
        self.assertFalse(future.done())  # ROS services have no generic error response.
        good = self.clients[1].call_async(AddTwoInts.Request(a=1, b=2))
        self.assertTrue(self.pump(5, good.done)); self.assertEqual(good.result().sum, 3)
        client.remove_pending_request(future)
        self.change(0, self.a_config); self.change(1, self.b_config)
        self.check_processes()

    @classmethod
    def cleanup(cls):
        cls.dump_logs()
        for process in cls.processes:
            process.send_signal(signal.SIGINT)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill(); process.wait()
        for executor, node, context in zip(cls.executors, cls.nodes, cls.contexts):
            executor.shutdown(); node.destroy_node(); context.shutdown()
        for log in cls.logs:
            log.close()
        cls.temp.cleanup()


if __name__ == "__main__":
    unittest.main(verbosity=2)
