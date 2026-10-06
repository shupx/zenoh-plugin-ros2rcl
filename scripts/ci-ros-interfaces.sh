#!/usr/bin/env bash
set -eo pipefail

workspace="${1:?usage: ci-ros-interfaces.sh WORKSPACE}"
mkdir -p "$workspace/src"

checkout() {
  local name="$1" url="$2" revision="$3"
  git init "$workspace/src/$name"
  git -C "$workspace/src/$name" fetch --depth 1 "$url" "$revision"
  git -C "$workspace/src/$name" checkout --detach FETCH_HEAD
}

checkout rosidl_rust https://github.com/ros2-rust/rosidl_rust.git \
  19d57818dab3b51e418c0893b70b3a1a8b64c495
checkout rcl_interfaces https://github.com/ros2/rcl_interfaces.git \
  82776fc9068d1e0cd4af11c2e2700195994b2eb1
checkout unique_identifier_msgs https://github.com/ros2/unique_identifier_msgs.git \
  27767cefcf8a80da44641dc208c57722c28aa11c

source /opt/ros/humble/setup.bash
cd "$workspace"
colcon build --merge-install --packages-select rosidl_generator_rs \
  --allow-overriding rosidl_generator_rs \
  --cmake-args -DBUILD_TESTING=OFF
source install/setup.bash
colcon build --merge-install \
  --packages-select builtin_interfaces unique_identifier_msgs action_msgs rcl_interfaces rosgraph_msgs \
  --allow-overriding builtin_interfaces unique_identifier_msgs action_msgs rcl_interfaces rosgraph_msgs \
  --cmake-args -DBUILD_TESTING=OFF

for package in builtin_interfaces unique_identifier_msgs action_msgs rcl_interfaces rosgraph_msgs; do
  test -f "install/share/$package/rust/Cargo.toml"
done
