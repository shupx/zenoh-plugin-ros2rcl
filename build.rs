fn main() {
    println!("cargo:rerun-if-changed=native/bridge.c");
    println!("cargo:rerun-if-env-changed=AMENT_PREFIX_PATH");
    println!("cargo:rerun-if-env-changed=ROS_DISTRO");
    let mut build = cc::Build::new();
    build.file("native/bridge.c").flag("-std=c11");
    for prefix in std::env::var("AMENT_PREFIX_PATH")
        .expect("Source ROS 2 setup.bash")
        .split(':')
    {
        let include = std::path::Path::new(prefix).join("include");
        build.include(&include);
        for package in [
            "rcl",
            "rmw",
            "rcutils",
            "rosidl_runtime_c",
            "rosidl_typesupport_interface",
            "rcl_yaml_param_parser",
            "type_description_interfaces",
            "builtin_interfaces",
            "service_msgs",
            "rosidl_dynamic_typesupport",
        ] {
            build.include(include.join(package));
        }
        println!("cargo:rustc-link-search=native={prefix}/lib");
    }
    build.compile("ros2rcl_native");
    for lib in ["rcl", "rmw", "rcutils", "dl"] {
        println!("cargo:rustc-link-lib={lib}");
    }
}
