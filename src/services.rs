//! Host services (broker, gateway, push proxy) as systemd user units.

use crate::config::{GlobalConfig, RepoConfig};
use crate::consts::{
    BROKER_URL, BUN_PATH, GATEWAY_CFG_PATH, OMP_PATH, PUSH_CFG_PATH, SANDBOX_PATH, UNIT_BROKER,
    UNIT_GATEWAY, UNIT_PROXY,
};
use crate::ui::{print_info, print_warning};
use crate::utils::{home_path, run_cmd, write_atomic};
use anyhow::{Context, Result, bail, ensure};
use std::fs::{create_dir_all, remove_file};
use std::process::{Command, Stdio};
use std::time::Duration;
use ureq::Agent;

fn systemctl_user(args: &[&str]) -> bool {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn unit_active(unit: &str) -> bool {
    systemctl_user(&["is-active", "--quiet", &format!("{unit}.service")])
}

fn http_status(url: &str) -> Option<u16> {
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(2)))
        .http_status_as_error(false)
        .build()
        .into();
    agent.get(url).call().ok().map(|r| r.status().as_u16())
}

fn wait_http(url: &str, want: u16, secs: u32) -> bool {
    for _ in 0..secs {
        if http_status(url) == Some(want) {
            return true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    false
}

fn port_free(port: u16) -> bool {
    http_status(&format!("http://127.0.0.1:{port}/")).is_none()
}

fn run_unit(unit: &str, env: &[(&str, &str)], cmd: &[&str]) -> Result<()> {
    systemctl_user(&["reset-failed", &format!("{unit}.service")]);
    let home = std::env::var("HOME")?;
    let path = format!("{home}/.bun/bin:/usr/local/bin:/usr/bin:/bin");
    let sandbox = home_path(SANDBOX_PATH)?;

    let mut c = Command::new("systemd-run");
    c.args([
        "--user",
        "--quiet",
        &format!("--unit={unit}"),
        "-p",
        "TimeoutStopSec=60",
    ])
    .arg(format!("--setenv=HOME={home}"))
    .arg(format!("--setenv=PATH={path}"))
    .arg(format!("--working-directory={}", sandbox.display()));
    for (k, v) in env {
        c.arg(format!("--setenv={k}={v}"));
    }
    let status = c
        .arg("--")
        .args(cmd)
        .status()
        .context("failed to run `systemd-run`")?;
    if !status.success() {
        bail!("systemd-run {unit} failed");
    }
    Ok(())
}

pub fn start_services(global: &GlobalConfig, repo: &RepoConfig) -> Result<()> {
    let omp = home_path(OMP_PATH)?.to_string_lossy().into_owned();
    let gateway_cfg = home_path(GATEWAY_CFG_PATH)?;
    let push_cfg = home_path(PUSH_CFG_PATH)?;

    if !unit_active(UNIT_BROKER) {
        ensure!(port_free(8765), "port 8765 is in use by something else");
        run_unit(
            UNIT_BROKER,
            &[],
            &[&omp, "auth-broker", "serve", "--bind=127.0.0.1:8765"],
        )?;
        ensure!(
            wait_http(&format!("{BROKER_URL}/v1/healthz"), 200, 30),
            "broker did not start; journalctl --user -u {UNIT_BROKER}"
        );
    }

    if !unit_active(UNIT_GATEWAY) {
        ensure!(port_free(4000), "port 4000 is in use by something else");
        create_dir_all(&gateway_cfg)?;
        let clients = gateway_cfg.join("clients.json");
        if !clients.exists() {
            write_atomic(&clients, "{}\n")?;
        }
        let clients_arg = format!("--clients={}", clients.display());
        run_unit(
            UNIT_GATEWAY,
            &[("OMP_AUTH_BROKER_URL", BROKER_URL)],
            &[
                &omp,
                "auth-gateway",
                "serve",
                "--bind=127.0.0.1:4000",
                &clients_arg,
            ],
        )?;
        ensure!(
            wait_http("http://127.0.0.1:4000/healthz", 200, 30),
            "gateway did not start; journalctl --user -u {UNIT_GATEWAY}"
        );
    }

    if !unit_active(UNIT_PROXY) {
        ensure!(port_free(4100), "port 4100 is in use by something else");
        create_dir_all(&push_cfg)?;
        // A fresh proxy client credential per proxy start.
        let token = run_cmd("openssl", &["rand", "-hex", "32"])?
            .trim()
            .to_string();
        let token_file = push_cfg.join("client.token");
        write_atomic(&token_file, &format!("{token}\n"))?;
        write_atomic(
            &push_cfg.join("client.gitconfig"),
            &format!(
                "[http \"http://localhost:4100/\"]\n\textraHeader = Authorization: Bearer {token}\n"
            ),
        )?;

        let install_id = global
            .app
            .installation_id
            .context("no App installation; run kafes add")?
            .to_string();
        let kafes_fork = repo.kafes_fork(global);
        let token_path = token_file.to_string_lossy();
        let key_path = push_cfg.join("app.pem");
        let key_path = key_path.to_string_lossy();
        let bun = home_path(BUN_PATH)?;
        let bun = bun.to_string_lossy();

        run_unit(
            UNIT_PROXY,
            &[
                ("KAFES_PUSH_REPO", &kafes_fork),
                ("KAFES_APP_CLIENT_ID", &global.app.client_id),
                ("KAFES_APP_INSTALLATION_ID", &install_id),
                ("KAFES_PUSH_CLIENT_TOKEN_FILE", &token_path),
                ("KAFES_APP_KEY", &key_path),
            ],
            &[
                &bun,
                "--no-env-file",
                "--config=./bunfig.toml",
                "./kafes-push-proxy.ts",
            ],
        )?;
        // 403 for an unauthenticated, disallowed path: the listener is up.
        ensure!(
            wait_http("http://127.0.0.1:4100/", 403, 30),
            "push proxy did not start; journalctl --user -u {UNIT_PROXY}"
        );
    }

    Ok(())
}

fn stop_proxy() -> bool {
    if !unit_active(UNIT_PROXY) {
        return true;
    }
    print_info("stopping the push proxy (revoking tokens; up to ~30 s)");
    let svc = format!("{UNIT_PROXY}.service");
    systemctl_user(&["stop", &svc]);
    let result = run_cmd(
        "systemctl",
        &["--user", "show", "-p", "Result", "--value", &svc],
    )
    .unwrap_or_default();
    systemctl_user(&["reset-failed", &svc]);
    if result.trim() == "success" {
        print_info("push proxy stopped cleanly");
        true
    } else {
        print_warning(
            "revocation or a mint failed: an installation token may stay valid for up to 1 h",
        );
        false
    }
}

pub fn stop_services() -> Result<()> {
    let clean = stop_proxy();
    let push_cfg = home_path(PUSH_CFG_PATH)?;
    let _ = remove_file(push_cfg.join("client.token"));
    let _ = remove_file(push_cfg.join("client.gitconfig"));
    for unit in [UNIT_GATEWAY, UNIT_BROKER] {
        let svc = format!("{unit}.service");
        if unit_active(unit) {
            systemctl_user(&["stop", &svc]);
        }
        systemctl_user(&["reset-failed", &svc]);
    }
    ensure!(clean, "the push proxy did not stop cleanly");
    Ok(())
}
