use crate::constants::GATEWAY_CFG_PATH;
use crate::constants::{PUSH_CFG_PATH, SANDBOX_PATH, STATE_PATH, WS_HOME};
use crate::profile::load_profile;
use crate::ui::print_info;
use crate::utils::{home_path, run_cmd, write_atomic};
use crate::workshop::{ws, ws_exec_stdin, ws_output};
use anyhow::{Result, ensure, Context};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::read_to_string;

fn agent_launcher(repo: &str) -> String {
    format!(
        r#"#!/bin/sh
# Rewritten by kafes on every task new, start and shell; edits don't last
exec "{WS_HOME}/.local/bin/omp" \
  --cwd "{WS_HOME}/src/{repo}" \
  --config "{WS_HOME}/.config/kafes-policy.yml" \
  --approval-mode always-ask \
  --tools read,grep,glob,edit,write,bash \
  --no-lsp --no-extensions --no-skills \
  --append-system-prompt "{WS_HOME}/.config/kafes-prompt.md" \
  "$@"
"#
    )
}

pub fn push_agent_files(task: &str, repo: &str) -> Result<()> {
    let sandbox = home_path(SANDBOX_PATH)?;
    let state = home_path(STATE_PATH)?;
    let files = [
        (
            "cat > ~/.config/kafes-policy.yml",
            read_to_string(sandbox.join("kafes-policy.yml"))?,
        ),
        // From the repository's profile, the same rules the check enforces
        ("cat > ~/.config/kafes-prompt.md", load_profile(repo)?.prompt()),
        (
            "cat > ~/.omp/agent/models.yml",
            read_to_string(state.join("task-models.yml"))?,
        ),
        (
            "cat > ~/.omp/agent/config.yml",
            read_to_string(state.join("task-config.yml"))?,
        ),
        (
            "mkdir -p ~/.local/bin && cat > ~/.local/bin/kafes-omp && chmod 755 ~/.local/bin/kafes-omp",
            agent_launcher(repo),
        ),
    ];
    for (script, content) in files {
        ws_exec_stdin(task, script, &content)?;
    }
    Ok(())
}

pub fn push_credentials(task: &str, providers: &[String]) -> Result<()> {
    let gitconfig = read_to_string(home_path(PUSH_CFG_PATH)?.join("client.gitconfig"))?;
    ws_exec_stdin(
        task,
        "umask 077; cat > ~/.config/kafes-push.gitconfig",
        &gitconfig,
    )?;
    let include = format!("{WS_HOME}/.config/kafes-push.gitconfig");
    ws(
        task,
        &[
            "exec",
            "dev",
            "--",
            "git",
            "config",
            "--global",
            "include.path",
            &include,
        ],
    )?;

    let token = run_cmd("openssl", &["rand", "-hex", "32"])?
        .trim()
        .to_string();
    let digest: String = Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let models: Vec<String> = providers.iter().map(|p| format!("{p}/*")).collect();
    let clients = json!({ "kafes-task": { "tokenSha256": digest, "models": models } });
    write_atomic(
        &home_path(GATEWAY_CFG_PATH)?.join("clients.json"),
        &format!("{clients}\n"),
    )?;

    ws_exec_stdin(
        task,
        "umask 077; cat > ~/.config/kafes-gateway.token",
        &token,
    )?;

    let code = ws_output(
        task,
        &[
            "exec",
            "dev",
            "--",
            "curl",
            "-sS",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "http://127.0.0.1:4000/healthz",
        ],
    )
    .context("the gateway is not reachable from the workshop")?;
    ensure!(
        code.trim() == "200",
        "the gateway is not reachable from the workshop"
    );
    print_info("credentials issued; the gateway loads its token within 10 s");
    Ok(())
}
