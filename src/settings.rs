use crate::constants::{OMP_PATH, SANDBOX_PATH, STATE_PATH, WS_HOME};
use crate::ui::{print_info, print_warning};
use crate::utils::{home_path, run_cmd_in, write_atomic};
use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::fs::create_dir_all;
use std::time::Duration;
use ureq::Agent;

const EXACT_KEYS: [&str; 15] = [
    "modelRoles",
    "modelPresets",
    "modelTags",
    "modelProviderOrder",
    "cycleOrder",
    "enabledModels",
    "disabledProviders",
    "defaultThinkingLevel",
    "hideThinkingBlock",
    "expandThinkingBlocks",
    "personality",
    "includeModelInPrompt",
    "colorBlindMode",
    "completion.notify",
    "images.blockImages",
];
const PREFIX_KEYS: [&str; 5] = [
    "retry.",
    "thinkingBudgets.",
    "display.",
    "statusLine.",
    "theme.",
];
const DEFAULT_PROVIDERS: [&str; 2] = ["github-copilot", "openrouter"];

fn allowed(key: &str) -> bool {
    EXACT_KEYS.contains(&key)
        || PREFIX_KEYS
            .iter()
            .any(|p| key.len() > p.len() && key.starts_with(p))
        || key
            .strip_prefix("compaction.")
            .is_some_and(|rest| !rest.is_empty() && !rest.starts_with("remote"))
}

/// True if any string anywhere in `v` starts with `!`, which OMP runs as a shell command
fn runs_shell(v: &Value) -> bool {
    match v {
        Value::String(s) => s.starts_with('!'),
        Value::Array(a) => a.iter().any(runs_shell),
        Value::Object(o) => o.values().any(runs_shell),
        _ => false,
    }
}

fn set_path(root: &mut Map<String, Value>, path: &str, value: Value) {
    let mut parts: Vec<&str> = path.split('.').collect();
    let Some(last) = parts.pop() else { return };
    let mut cur = root;
    for p in parts {
        let entry = cur.entry(p).or_insert_with(|| json!({}));
        if !entry.is_object() {
            *entry = json!({});
        }
        let Value::Object(next) = entry else {
            unreachable!()
        };
        cur = next;
    }
    cur.insert(last.to_string(), value);
}

pub struct HostSettings {
    pub copied: Value,
    pub dropped: Vec<String>,
}

fn host_settings() -> Result<HostSettings> {
    let omp = home_path(OMP_PATH)?.to_string_lossy().into_owned();
    let sandbox = home_path(SANDBOX_PATH)?;
    let out = run_cmd_in(
        Some(&sandbox),
        &omp,
        &["config", "list", "--scope", "global", "--json"],
    )
    .context("omp config list failed on the host")?;
    let list: Map<String, Value> =
        serde_json::from_str(&out).context("omp config list: bad JSON")?;

    let mut copied = Map::new();
    let mut dropped = Vec::new();
    for (key, entry) in list {
        let value = entry.get("value");
        let ok = entry.is_object()
            && value.is_some()
            && entry["redacted"] != true
            && allowed(&key)
            && !value.is_some_and(runs_shell);
        if !ok {
            dropped.push(key);
            continue;
        }
        let mut value = value.cloned().unwrap_or(Value::Null);
        if matches!(key.as_str(), "enabledModels" | "disabledProviders")
            && let Value::Array(items) = &mut value
        {
            items.retain(Value::is_string);
        }
        set_path(&mut copied, &key, value);
    }
    Ok(HostSettings {
        copied: Value::Object(copied),
        dropped,
    })
}

fn collect_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        Value::Object(o) => o.values().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

fn referenced_providers(config: &Value) -> BTreeSet<String> {
    let mut found = Vec::new();
    collect_strings(&config["modelRoles"], &mut found);
    if let Some(presets) = config["modelPresets"].as_object() {
        for preset in presets.values() {
            collect_strings(&preset["modelRoles"], &mut found);
        }
    }
    if let Some(chains) = config["retry"]["fallbackChains"].as_object() {
        for (k, v) in chains {
            found.push(k.clone());
            collect_strings(v, &mut found);
        }
    }
    found
        .iter()
        .flat_map(|s| s.split(','))
        .filter_map(|s| s.trim().split_once('/'))
        .map(|(provider, _)| provider.to_string())
        .collect()
}

fn served_providers() -> Result<BTreeSet<String>> {
    let omp = home_path(OMP_PATH)?.to_string_lossy().into_owned();
    let sandbox = home_path(SANDBOX_PATH)?;
    let token = run_cmd_in(Some(&sandbox), &omp, &["auth-gateway", "token"])
        .context("can't read the gateway token")?;
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let models: Value = agent
        .get("http://127.0.0.1:4000/v1/models")
        .header("Authorization", &format!("Bearer {}", token.trim()))
        .call()
        .context("can't list the gateway's models")?
        .body_mut()
        .read_json()?;
    let data = models["data"]
        .as_array()
        .context("gateway models: no data")?;
    Ok(data
        .iter()
        .filter_map(|m| m["id"].as_str()?.split('/').next())
        .map(str::to_string)
        .collect())
}

pub struct TaskSettings {
    pub config: Value,
    pub providers: Vec<String>,
}

fn valid_provider(p: &str) -> bool {
    p.chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && p.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn prepare_task_settings() -> Result<TaskSettings> {
    let host = host_settings()?;
    let served = served_providers()?;
    let referenced = referenced_providers(&host.copied);

    let mut providers: Vec<String> = Vec::new();
    let candidates = DEFAULT_PROVIDERS
        .iter()
        .map(|p| p.to_string())
        .chain(referenced.iter().cloned());
    for p in candidates {
        if !valid_provider(&p) || providers.contains(&p) {
            continue;
        }
        if served.contains(&p) {
            providers.push(p);
        } else if referenced.contains(&p) {
            print_warning(&format!(
                "your settings use '{p}', which the host gateway doesn't serve, so roles and fallbacks \
                 on it will fail in the workshop. Log it in on the host (omp auth-broker login {p})"
            ));
        }
    }
    ensure!(
        !providers.is_empty(),
        "the host gateway serves none of the providers your settings use"
    );

    print_info(&format!("host settings copied: {}", summary(&host.copied)));
    let dropped = if host.dropped.is_empty() {
        "none".to_string()
    } else {
        host.dropped.join(", ")
    };
    print_info(&format!("host settings kept back: {dropped}"));
    print_info(&format!(
        "providers routed through the gateway: {}",
        providers.join(" ")
    ));
    Ok(TaskSettings {
        config: host.copied,
        providers,
    })
}

fn summary(config: &Value) -> String {
    let Some(obj) = config.as_object() else {
        return "none".into();
    };
    let keys: Vec<String> = obj
        .iter()
        .flat_map(|(k, v)| match v.as_object() {
            Some(sub) if !sub.is_empty() => sub.keys().map(|s| format!("{k}.{s}")).collect(),
            _ => vec![k.clone()],
        })
        .collect();
    if keys.is_empty() {
        "none".into()
    } else {
        keys.join(", ")
    }
}

fn models_yml(providers: &[String]) -> String {
    let mut out = String::from(
        "# Generated by kafes: every provider goes through the host gateway\nproviders:\n",
    );
    for p in providers {
        out.push_str(&format!(
            "  {p}:\n    transport: pi-native\n    baseUrl: http://127.0.0.1:4000\n    \
             apiKey: \"!cat {WS_HOME}/.config/kafes-gateway.token\"\n"
        ));
    }
    out
}

pub fn save_task_settings(s: &TaskSettings) -> Result<()> {
    let state = home_path(STATE_PATH)?;
    create_dir_all(&state)?;
    write_atomic(&state.join("task-models.yml"), &models_yml(&s.providers))?;
    let config = format!(
        "# Generated by kafes from allowlisted host settings\n{}\n",
        serde_json::to_string_pretty(&s.config)?
    );
    write_atomic(&state.join("task-config.yml"), &config)
}
