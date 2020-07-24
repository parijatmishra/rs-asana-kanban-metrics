use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct MyConfig {
    pub projects: HashMap<String, MyProjectConfig>,
}

#[derive(Debug, Deserialize)]
pub struct MyProjectConfig {
    pub gid: String,
    pub horizon: DateTime<Utc>,
    pub cfd_states: Vec<String>,
    pub done_states: Vec<String>,
}

pub fn parse_config(config_str: &str) -> MyConfig {
    let config: MyConfig = serde_json::from_str(config_str).expect("Invalid config");
    return config;
}
