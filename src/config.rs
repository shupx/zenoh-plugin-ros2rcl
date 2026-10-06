use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub domain_id: usize,
    pub node_name: String,
    pub config_service: String,
    pub key_prefix: String,
    pub publish: Vec<Publish>,
    pub subscribe: Vec<Subscribe>,
    pub expose_services: Vec<ExposeService>,
    pub query_services: Vec<QueryService>,
    pub max_in_flight: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            domain_id: std::env::var("ROS_DOMAIN_ID")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(0),
            node_name: "zenoh_ros2rcl".into(),
            config_service: "/zenoh_ros2rcl/set_config".into(),
            key_prefix: "ros2".into(),
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
    pub topic: String,
    pub ros_type: String,
    #[serde(default)]
    pub max_frequency: Option<f64>,
    #[serde(default)]
    pub qos: Qos,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Subscribe {
    pub key: String,
    pub topic: String,
    pub ros_type: String,
    #[serde(default)]
    pub qos: Qos,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExposeService {
    pub service: String,
    pub ros_type: String,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct QueryService {
    pub key: String,
    pub service: String,
    pub ros_type: String,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
}
fn timeout() -> u64 {
    5000
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
pub fn key(s: &str) -> Result<()> {
    zenoh::key_expr::KeyExpr::try_from(s.to_owned())
        .map_err(|e| anyhow::anyhow!("invalid key {s}: {e}"))?;
    ensure!(!s.contains('*'), "route keys must be concrete: {s}");
    Ok(())
}
impl Config {
    pub fn export_key(&self, name: &str) -> String {
        if self.key_prefix.is_empty() {
            name.trim_start_matches('/').into()
        } else {
            format!("{}/{}", self.key_prefix, name.trim_start_matches('/'))
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.domain_id <= 232, "domain_id must be 0..232");
        ensure!(identifier(&self.node_name), "invalid node_name");
        ros_name(&self.config_service)?;
        if !self.key_prefix.is_empty() {
            key(&self.key_prefix)?;
        }
        ensure!(
            self.max_in_flight > 0 && self.max_in_flight <= 65536,
            "max_in_flight must be 1..65536"
        );
        let mut exports = HashSet::new();
        let mut inputs = HashSet::new();
        for r in &self.publish {
            ros_name(&r.topic)?;
            type_parts(&r.ros_type, "msg")?;
            key(&self.export_key(&r.topic))?;
            ensure!(r.qos.depth > 0, "QoS depth must be positive");
            if let Some(f) = r.max_frequency {
                ensure!(
                    f.is_finite() && f > 0.0,
                    "max_frequency must be finite and positive"
                );
            }
            ensure!(
                exports.insert(self.export_key(&r.topic)),
                "duplicate exported key"
            );
        }
        for r in &self.subscribe {
            ros_name(&r.topic)?;
            type_parts(&r.ros_type, "msg")?;
            key(&r.key)?;
            ensure!(r.qos.depth > 0, "QoS depth must be positive");
            ensure!(
                inputs.insert(r.topic.clone()),
                "duplicate subscribed ROS topic"
            );
            ensure!(
                !self.publish.iter().any(|p| p.topic == r.topic),
                "local topic feedback loop: {}",
                r.topic
            );
        }
        let mut services = HashSet::new();
        for r in &self.expose_services {
            ros_name(&r.service)?;
            type_parts(&r.ros_type, "srv")?;
            key(&self.export_key(&r.service))?;
            ensure!(r.timeout_ms > 0, "service timeout must be positive");
            ensure!(
                exports.insert(self.export_key(&r.service)),
                "duplicate exported key"
            );
            ensure!(
                r.service != self.config_service,
                "cannot expose config service"
            );
        }
        for r in &self.query_services {
            ros_name(&r.service)?;
            type_parts(&r.ros_type, "srv")?;
            key(&r.key)?;
            ensure!(r.timeout_ms > 0, "service timeout must be positive");
            ensure!(services.insert(&r.service), "duplicate local service");
            ensure!(r.service != self.config_service, "reserved config service");
            ensure!(
                !self.expose_services.iter().any(|e| e.service == r.service),
                "local service feedback loop"
            );
        }
        Ok(())
    }
    pub fn validate_update(&self, old: &Self) -> Result<()> {
        self.validate()?;
        if self.domain_id != old.domain_id
            || self.node_name != old.node_name
            || self.config_service != old.config_service
        {
            bail!("domain_id, node_name and config_service require restart");
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
        assert_eq!(c.export_key("/camera/image"), "ros2/camera/image");
        c.key_prefix.clear();
        assert_eq!(c.export_key("/x"), "x");
        c.publish.push(Publish {
            topic: "/x".into(),
            ros_type: "std_msgs/msg/String".into(),
            max_frequency: Some(10.0),
            qos: Qos::default(),
        });
        c.validate().unwrap();
        c.publish[0].max_frequency = Some(f64::NAN);
        assert!(c.validate().is_err());
        c.publish[0].max_frequency = None;
        c.subscribe.push(Subscribe {
            key: "remote/x".into(),
            topic: "/x".into(),
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
        assert!(key("a/**").is_err());
        let c = Config {
            domain_id: 233,
            ..Config::default()
        };
        assert!(c.validate().is_err());
    }
}
