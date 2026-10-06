#!/usr/bin/env python3
"""Package native release binaries using versioned Rust target names."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import zipfile


TARGETS = {"x86_64-unknown-linux-gnu": 62, "aarch64-unknown-linux-gnu": 183}
ROS_DISTROS = {"humble": "22.04", "jazzy": "24.04", "lyrical": "26.04"}
ROOT = Path(__file__).resolve().parents[1]


def verify_elf(path, target):
    with path.open("rb") as binary:
        header = binary.read(20)
    if len(header) != 20 or header[:6] != b"\x7fELF\x02\x01":
        raise SystemExit(f"Expected little-endian ELF64: {path}")
    if struct.unpack_from("<H", header, 18)[0] != TARGETS[target]:
        raise SystemExit(f"Architecture does not match {target}: {path}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True, choices=TARGETS)
    parser.add_argument("--ros-distro", required=True, choices=ROS_DISTROS)
    parser.add_argument("--build-dir", type=Path, default=ROOT / "target/release")
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    if os.environ.get("ROS_DISTRO") != args.ros_distro:
        parser.error("source the matching ROS environment before packaging")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", args.version):
        parser.error("invalid version")
    binaries = [args.build_dir / "libzenoh_plugin_ros2rcl.so", args.build_dir / "zenoh-bridge-ros2rcl"]
    for binary in binaries:
        verify_elf(binary, args.target)
        result = subprocess.run(["ldd", str(binary)], capture_output=True, text=True, check=True)
        if "not found" in result.stdout + result.stderr:
            raise SystemExit(f"Unresolved dependencies for {binary}:\n{result.stdout}{result.stderr}")
    metadata = {
        "version": args.version, "target": args.target, "ros_distro": args.ros_distro,
        "zenoh_version": "1.10.1", "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "runtime": f"Ubuntu {ROS_DISTROS[args.ros_distro]} / ROS 2 {args.ros_distro.title()}; source the ROS environment before use",
    }
    args.output.mkdir(parents=True, exist_ok=True)
    for binary, name in zip(binaries, ["zenoh-plugin-ros2rcl", "zenoh-bridge-ros2rcl"]):
        archive = args.output / f"{name}-{args.version}-{args.target}-ros2-{args.ros_distro}.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
            output.write(binary, binary.name)
            for filename in ["README.md", "LICENSE", "vendor/NOTICE"]:
                output.write(ROOT / filename, Path(filename).name)
            for filename in ["readme_dev.md", "TESTING.md", "vendor/README.md", "scripts/set_config.py"]:
                output.write(ROOT / filename, filename)
            for config in sorted((ROOT / "config").glob("*.json5")):
                output.write(config, f"config/{config.name}")
            output.writestr("build-info.json", json.dumps(metadata, indent=2) + "\n")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_suffix(archive.suffix + ".sha256").write_text(f"{digest}  {archive.name}\n")
        print(archive)


if __name__ == "__main__":
    main()
