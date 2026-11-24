use crate::constants::{BUN_PATH, BUN_VERSION, OMP_PATH, OMP_VERSION};
use crate::ui::{self, step};
use crate::utils::{check_cmd, get_cmd_version, home_path, is_executable, run_cmd, run_status};
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

fn omp_version_ok(omp: &Path) -> bool {
    Command::new(omp)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == format!("omp/{OMP_VERSION}"))
        .unwrap_or(false)
}

pub fn setup_tools() -> Result<()> {
    ui::section("Host tools");

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
        if !ui::confirm_help("Install the LXD snap (6/stable)?", "requires sudo")? {
            bail!("LXD is required");
        }
        let status = Command::new("sudo")
            .args(["snap", "install", "--channel=6/stable", "lxd"])
            .status()
            .context("cannot run sudo")?;
        if !status.success() {
            bail!("cannot install the lxd snap");
        }
    }

    if run_cmd("snap", &["list", "workshop"]).is_err() {
        if !ui::confirm_help("Install the Workshop snap?", "requires sudo")? {
            bail!("Workshop is required");
        }
        let status = Command::new("sudo")
            .args(["snap", "install", "--classic", "workshop"])
            .status()
            .context("cannot run sudo")?;
        if !status.success() {
            bail!("cannot install the workshop snap");
        }
    }

    run_status("sudo", &["lxd", "waitready"])?;
    ui::done("lxd is ready");
    check_cmd("workshop")?;

    let bun = home_path(BUN_PATH)?;
    if !is_executable(&bun) {
        if !ui::confirm(&format!("Install Bun {BUN_VERSION} into ~/.bun?"))? {
            bail!("Bun is required");
        }
        let status = Command::new("bash")
            .args(["-o", "pipefail", "-c"])
            .arg(format!(
                "curl -fsSL https://bun.sh/install | bash -s bun-v{BUN_VERSION}"
            ))
            .status()?;
        if !status.success() {
            bail!("cannot install Bun");
        }
    }

    let omp = home_path(OMP_PATH)?;
    if !omp_version_ok(&omp) {
        if !ui::confirm(&format!("Install OMP {OMP_VERSION} globally with Bun?"))? {
            bail!("OMP is required");
        }
        let status = Command::new(&bun)
            .args(["install", "--global"])
            .arg(format!("@oh-my-pi/pi-coding-agent@{OMP_VERSION}"))
            .status()?;
        if !status.success() {
            bail!("cannot install OMP");
        }
        if !omp_version_ok(&omp) {
            bail!("OMP version mismatch");
        }
    }
    ui::done(&format!("bun {BUN_VERSION} and omp {OMP_VERSION}"));

    Ok(())
}

