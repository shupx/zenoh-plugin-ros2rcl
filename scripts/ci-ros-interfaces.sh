#!/usr/bin/env bash
set -eo pipefail

workspace="${1:?usage: ci-ros-interfaces.sh WORKSPACE [ROS_DISTRO]}"
distro="${2:-${ROS_DISTRO:-humble}}"
packages=(builtin_interfaces unique_identifier_msgs action_msgs rcl_interfaces rosgraph_msgs)
case "$distro" in
  humble)
    rcl_interfaces_revision=82776fc9068d1e0cd4af11c2e2700195994b2eb1
    unique_identifier_msgs_revision=27767cefcf8a80da44641dc208c57722c28aa11c
    ;;
  jazzy)
    packages+=(service_msgs)
    rcl_interfaces_revision=7aa3caf43377ea6ad615bc1040832e2c7566bfbe
    unique_identifier_msgs_revision=901599ee92c2ee949f42a5c5d821c92b3e515d69
    ;;
  lyrical)
    packages+=(service_msgs)
    rcl_interfaces_revision=b90e36e4adf5ed878efa2f365a84798f782618cd
    unique_identifier_msgs_revision=b4779af6b3d3c45e710802a0e7f86113da281ae8
    ;;
  *) echo "Unsupported ROS distribution: $distro" >&2; exit 1 ;;
esac
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
  "$rcl_interfaces_revision"
checkout unique_identifier_msgs https://github.com/ros2/unique_identifier_msgs.git \
  "$unique_identifier_msgs_revision"

source "/opt/ros/$distro/setup.bash"
cd "$workspace"
colcon build --merge-install --packages-select rosidl_generator_rs \
  --allow-overriding rosidl_generator_rs \
  --cmake-args -DBUILD_TESTING=OFF
source install/setup.bash
colcon build --merge-install \
  --packages-select "${packages[@]}" \
  --allow-overriding "${packages[@]}" \
  --cmake-args -DBUILD_TESTING=OFF

for package in "${packages[@]}"; do
  test -f "install/share/$package/rust/Cargo.toml"
done
