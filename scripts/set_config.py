#!/usr/bin/env python3
"""Atomically replace forwarding configuration through the local ROS service."""
import argparse
import json
from pathlib import Path
import rclpy
from rcl_interfaces.msg import Parameter, ParameterValue
from rcl_interfaces.srv import SetParametersAtomically


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("config", type=Path, help="strict JSON configuration")
    parser.add_argument("--service", default="/zenoh_ros2rcl/set_config")
    parser.add_argument("--timeout", type=float, default=15)
    args = parser.parse_args()
    value = json.loads(args.config.read_text())
    rclpy.init()
    node = rclpy.create_node("ros2rcl_config_client")
    try:
        client = node.create_client(SetParametersAtomically, args.service)
        if not client.wait_for_service(timeout_sec=args.timeout):
            raise SystemExit("configuration service unavailable")
        req = SetParametersAtomically.Request(parameters=[Parameter(name="config", value=ParameterValue(type=4, string_value=json.dumps(value)))])
        future = client.call_async(req)
        rclpy.spin_until_future_complete(node, future, timeout_sec=args.timeout)
        if not future.done():
            raise SystemExit("configuration service timed out")
        result = future.result().result
        print(result.reason)
        if not result.successful:
            raise SystemExit(1)
    finally:
        node.destroy_node()
        rclpy.shutdown()


if __name__ == "__main__":
    main()
