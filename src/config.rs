use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub node_name: String,
    pub config_service: String,
    pub publish: Vec<Publish>,
    pub subscribe: Vec<Subscribe>,
    pub expose_services: Vec<ExposeService>,
    pub query_services: Vec<QueryService>,
    pub max_in_flight: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            node_name: "zenoh_ros2rcl".into(),
            config_service: "/zenoh_ros2rcl/set_config".into(),
            publish: vec![],
            subscribe: vec![],
            expose_services: vec![],
            query_services: vec![],
            max_in_flight: 64,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Publish {
    pub ros_topic: String,
    #[serde(default = "default_prefix")]
    pub zenoh_key_prefix: String,
    pub ros_type: String,
    #[serde(default)]
    pub max_frequency: Option<f64>,
    #[serde(default)]
    pub qos: Qos,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Subscribe {
    pub zenoh_key: String,
    pub ros_topic: String,
    pub ros_type: String,
    #[serde(default)]
    pub qos: Qos,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExposeService {
    pub ros_service: String,
    #[serde(default = "default_prefix")]
    pub zenoh_key_prefix: String,
    pub ros_type: String,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct QueryService {
    pub zenoh_key: String,
    pub ros_service: String,
    pub ros_type: String,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
}
fn timeout() -> u64 {
    5000
}
fn default_prefix() -> String {
    "ros2".into()
}

pub fn domain_from_env() -> Result<usize> {
    match std::env::var("ROS_DOMAIN_ID") {
        Ok(value) => parse_domain(Some(&value)),
        Err(std::env::VarError::NotPresent) => parse_domain(None),
        Err(e) => Err(anyhow::anyhow!("invalid ROS_DOMAIN_ID: {e}")),
    }
}
fn parse_domain(value: Option<&str>) -> Result<usize> {
    let domain = match value {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("ROS_DOMAIN_ID must be 0..232"))?,
        None => 0,
    };
    ensure!(domain <= 232, "ROS_DOMAIN_ID must be 0..232");
    Ok(domain)
}

fn export_key(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.trim_start_matches('/').into()
    } else {
        format!("{prefix}/{}", name.trim_start_matches('/'))
    }
}
impl Publish {
    pub fn zenoh_key(&self) -> String {
        export_key(&self.zenoh_key_prefix, &self.ros_topic)
    }
}
impl ExposeService {
    pub fn zenoh_key(&self) -> String {
        export_key(&self.zenoh_key_prefix, &self.ros_service)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Qos {
    pub reliable: bool,
    pub transient_local: bool,
    pub depth: u32,
}
impl Default for Qos {
    fn default() -> Self {
        Self {
            reliable: true,
            transient_local: false,
            depth: 10,
        }
    }
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && !s.as_bytes()[0].is_ascii_digit()
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}
pub fn ros_name(name: &str) -> Result<()> {
    ensure!(
        name.starts_with('/') && name.len() > 1 && name[1..].split('/').all(identifier),
        "invalid absolute ROS name: {name}"
    );
    Ok(())
}
pub fn type_parts<'a>(s: &'a str, ns: &str) -> Result<(&'a str, &'a str)> {
    let p: Vec<_> = s.split('/').collect();
    ensure!(
        p.len() == 3 && p[1] == ns && identifier(p[0]) && identifier(p[2]),
        "expected package/{ns}/Type: {s}"
    );
    Ok((p[0], p[2]))
}
pub fn zenoh_key(s: &str) -> Result<()> {
    zenoh::key_expr::KeyExpr::try_from(s.to_owned())
        .map_err(|e| anyhow::anyhow!("invalid zenoh_key {s}: {e}"))?;
    ensure!(!s.contains('*'), "route keys must be concrete: {s}");
    Ok(())
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(identifier(&self.node_name), "invalid node_name");
        ros_name(&self.config_service)?;
        ensure!(
            self.max_in_flight > 0 && self.max_in_flight <= 65536,
            "max_in_flight must be 1..65536"
        );
        let mut exports = HashSet::new();
        let mut inputs = HashSet::new();
        for r in &self.publish {
            ros_name(&r.ros_topic)?;
            type_parts(&r.ros_type, "msg")?;
            if !r.zenoh_key_prefix.is_empty() {
                zenoh_key(&r.zenoh_key_prefix)?;
            }
            zenoh_key(&r.zenoh_key())?;
            ensure!(r.qos.depth > 0, "QoS depth must be positive");
            if let Some(f) = r.max_frequency {
                ensure!(
                    f.is_finite() && f > 0.0,
                    "max_frequency must be finite and positive"
                );
            }
            ensure!(
                exports.insert(r.zenoh_key()),
                "duplicate exported zenoh_key"
            );
        }
        for r in &self.subscribe {
            ros_name(&r.ros_topic)?;
            type_parts(&r.ros_type, "msg")?;
            zenoh_key(&r.zenoh_key)?;
            ensure!(r.qos.depth > 0, "QoS depth must be positive");
            ensure!(
                inputs.insert(r.ros_topic.clone()),
                "duplicate subscribed ROS topic"
            );
            ensure!(
                !self.publish.iter().any(|p| p.ros_topic == r.ros_topic),
                "local topic feedback loop: {}",
                r.ros_topic
            );
        }
        let mut services = HashSet::new();
        for r in &self.expose_services {
            ros_name(&r.ros_service)?;
            type_parts(&r.ros_type, "srv")?;
            if !r.zenoh_key_prefix.is_empty() {
                zenoh_key(&r.zenoh_key_prefix)?;
            }
            zenoh_key(&r.zenoh_key())?;
            ensure!(r.timeout_ms > 0, "service timeout must be positive");
            ensure!(
                exports.insert(r.zenoh_key()),
                "duplicate exported zenoh_key"
            );
            ensure!(
                r.ros_service != self.config_service,
                "cannot expose config service"
            );
        }
        for r in &self.query_services {
            ros_name(&r.ros_service)?;
            type_parts(&r.ros_type, "srv")?;
            zenoh_key(&r.zenoh_key)?;
            ensure!(r.timeout_ms > 0, "service timeout must be positive");
            ensure!(services.insert(&r.ros_service), "duplicate local service");
            ensure!(
                r.ros_service != self.config_service,
                "reserved config service"
            );
            ensure!(
                !self
                    .expose_services
                    .iter()
                    .any(|e| e.ros_service == r.ros_service),
                "local service feedback loop"
            );
        }
        Ok(())
    }
    pub fn validate_update(&self, old: &Self) -> Result<()> {
        self.validate()?;
        if self.node_name != old.node_name || self.config_service != old.config_service {
            bail!("node_name and config_service require restart");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_and_validation() {
        let mut c = Config::default();
        assert_eq!(export_key("ros2", "/camera/image"), "ros2/camera/image");
        assert_eq!(export_key("", "/x"), "x");
        c.publish.push(Publish {
            ros_topic: "/x".into(),
            zenoh_key_prefix: "ros2".into(),
            ros_type: "std_msgs/msg/String".into(),
            max_frequency: Some(10.0),
            qos: Qos::default(),
        });
        c.validate().unwrap();
        c.publish[0].max_frequency = Some(f64::NAN);
        assert!(c.validate().is_err());
        c.publish[0].max_frequency = None;
        c.subscribe.push(Subscribe {
            zenoh_key: "remote/x".into(),
            ros_topic: "/x".into(),
            ros_type: "std_msgs/msg/String".into(),
            qos: Qos::default(),
        });
        assert!(c.validate().is_err());
    }
    #[test]
    fn rejects_bad_config() {
        assert!(serde_json::from_str::<Config>(r#"{"publsh": []}"#).is_err());
        for n in ["relative", "/", "/a//b", "/1a", "/a~"] {
            assert!(ros_name(n).is_err());
        }
        assert!(zenoh_key("a/**").is_err());
        assert_eq!(parse_domain(None).unwrap(), 0);
        assert_eq!(parse_domain(Some("171")).unwrap(), 171);
        for value in ["233", "-1", "bad", ""] {
            assert!(parse_domain(Some(value)).is_err());
        }
        for config in [
            r#"{"domain_id": 10}"#,
            r#"{"key_prefix": "old"}"#,
            r#"{"publish": [{"topic": "/x", "ros_type": "std_msgs/msg/String"}]}"#,
        ] {
            assert!(serde_json::from_str::<Config>(config).is_err());
        }
    }

    #[test]
    fn per_route_prefixes_and_legacy_fields() {
        let mut c: Config = serde_json::from_str(r#"{
            "publish": [
                {"ros_topic": "/status", "ros_type": "std_msgs/msg/String"},
                {"ros_topic": "/image", "ros_type": "std_msgs/msg/String", "zenoh_key_prefix": "camera"}
            ],
            "expose_services": [{"ros_service": "/status", "ros_type": "std_srvs/srv/Trigger", "zenoh_key_prefix": "services"}]
        }"#).unwrap();
        c.validate().unwrap();
        assert_eq!(c.publish[0].zenoh_key(), "ros2/status");
        assert_eq!(c.publish[1].zenoh_key(), "camera/image");
        assert_eq!(c.expose_services[0].zenoh_key(), "services/status");
        c.expose_services[0].zenoh_key_prefix = "ros2".into();
        assert!(c.validate().is_err());
        c.expose_services[0].zenoh_key_prefix.clear();
        assert_eq!(c.expose_services[0].zenoh_key(), "status");
        c.validate().unwrap();
        c.publish[0].zenoh_key_prefix = "bad/*".into();
        assert!(c.validate().is_err());
        for config in [
            r#"{"subscribe": [{"key": "old/key", "topic": "/x", "ros_type": "std_msgs/msg/String"}]}"#,
            r#"{"expose_services": [{"service": "/x", "ros_type": "std_srvs/srv/Trigger"}]}"#,
            r#"{"query_services": [{"key": "old/key", "service": "/x", "ros_type": "std_srvs/srv/Trigger"}]}"#,
        ] {
            assert!(serde_json::from_str::<Config>(config).is_err());
        }
    }
}
