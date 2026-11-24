//! Anonymous reads of the public GitHub API.

use anyhow::{Context, Result};
use serde_json::Value;
use std::time::Duration;
use ureq::Agent;

pub fn gh_api(path: &str) -> Result<Value> {
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let url = format!("https://api.github.com{path}");
    let value = agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "kafes")
        .call()
        .with_context(|| format!("cannot read {url}"))?
        .body_mut()
        .read_json::<Value>()?;
    Ok(value)
}
