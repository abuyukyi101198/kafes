pub mod utils;

use anyhow::{Context, Result, bail};
use std::fs::create_dir_all;
use std::path::Path;
use std::process::Command;
use utils::{
    check_cmd, get_cmd_version, home_path, is_executable, print, print_info, prompt_confirm,
    prompt_multiselect, run_cmd, run_status, step,
};


const SANDBOX_PATH: &str = "kafes/sandbox";
const BUN_PATH: &str = ".bun/bin/bun";
const BUN_VERSION: &str = "1.4.4";
const OMP_PATH: &str = ".bun/bin/omp";
const OMP_VERSION: &str = "18.16.0";
const GATEWAY_CFG_PATH: &str = ".config/kafes/gateway";
const TASKS_PATH: &str = "kafes/tasks";

fn omp_version_ok(omp: &Path) -> bool {
    Command::new(omp)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == format!("omp/{OMP_VERSION}"))
        .unwrap_or(false)
}

fn setup_tools() -> Result<()> {
    print("Setting up host tools...");

    for tool in ["curl", "git", "openssl", "sha256sum", "systemd-run"] {
        check_cmd(tool)?;
    }

    check_cmd("systemctl")?;
    let systemd_state = Command::new("systemctl")
        .args(["--user", "is-system-running"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if systemd_state != "running" && systemd_state != "degraded" {
        bail!("no systemd user session; log in on a desktop or enable lingering");
    }

    step("curl", || get_cmd_version("curl", &["--version"], 1))?;
    step("git", || get_cmd_version("git", &["--version"], 2))?;
    step("openssl", || get_cmd_version("openssl", &["version"], 1))?;

    if run_cmd("snap", &["list", "lxd"]).is_err() {
        if !prompt_confirm("Do you want to install the LXD snap (6/stable)? This requires sudo.")? {
            bail!("LXD is required");
        }
        let status = Command::new("sudo")
            .args(["snap", "install", "--channel=6/stable", "lxd"])
            .status()
            .context("couldn't run sudo")?;
        if !status.success() {
            bail!("snap install lxd failed");
        }
    }

    if run_cmd("snap", &["list", "workshop"]).is_err() {
        if !prompt_confirm("Do you want to install the Workshop snap? This requires sudo.")? {
            bail!("Workshop is required");
        }
        let status = Command::new("sudo")
            .args(["snap", "install", "--classic", "workshop"])
            .status()
            .context("couldn't run sudo")?;
        if !status.success() {
            bail!("snap install workshop failed");
        }
    }

    run_status("sudo", &["lxd", "waitready"])?;
    print_info("lxd is ready");
    check_cmd("workshop")?;

    let bun = home_path(BUN_PATH)?;
    if !is_executable(&bun) {
        if !prompt_confirm(&format!(
            "Do you want to install Bun {BUN_VERSION} into ~/.bun?"
        ))? {
            bail!("Bun is required");
        }
        let status = Command::new("bash")
            .args(["-o", "pipefail", "-c"])
            .arg(format!(
                "curl -fsSL https://bun.sh/install | bash -s bun-v{BUN_VERSION}"
            ))
            .status()?;
        if !status.success() {
            bail!("bun installation failed");
        }
    }

    let omp = home_path(OMP_PATH)?;
    if !omp_version_ok(&omp) {
        if !prompt_confirm(&format!(
            "Do you want to install OMP {OMP_VERSION} globally with Bun?"
        ))? {
            bail!("OMP is required");
        }
        let status = Command::new(&bun)
            .args(["install", "--global"])
            .arg(format!("@oh-my-pi/pi-coding-agent@{OMP_VERSION}"))
            .status()?;
        if !status.success() {
            bail!("omp installation failed");
        }
        if !omp_version_ok(&omp) {
            bail!("OMP version mismatch");
        }
    }

    Ok(())
}

const PROVIDERS: [&str; 2] = ["github-copilot", "openrouter"];

fn setup_providers() -> Result<()> {
    print("Setting up host providers...");

    let chosen = prompt_multiselect(
        "Select providers to log in:",
        PROVIDERS.iter().map(|p| p.to_string()).collect(),
    )?;

    let omp = home_path(OMP_PATH)?;
    let omp = omp.to_string_lossy();
    for provider in chosen {
        run_status(
            "env",
            &[
                "-u",
                "OMP_AUTH_BROKER_URL",
                "-u",
                "OPENROUTER_API_KEY",
                &omp,
                "auth-broker",
                "login",
                provider.as_str(),
            ],
        )?;
    }

    let gateway_cfg = home_path(GATEWAY_CFG_PATH)?;
    create_dir_all(&gateway_cfg)?;
    let clients = gateway_cfg.join("clients.json");
    if !clients.exists() {
        std::fs::write(&clients, "{}\n")?;
    }
    create_dir_all(home_path(TASKS_PATH)?)?;
    create_dir_all(home_path(SANDBOX_PATH)?)?;
    Ok(())
}

fn main() {
    println!("Hello, world!");
}
