pub mod config;
pub mod utils;

use crate::config::{AppConfig, GlobalConfig, load_global_config, save_global_config, valid_name};
use crate::utils::{gh_api, prompt_instruction, prompt_text, write_atomic};
use anyhow::{Context, Result, bail};
use std::fs::{create_dir_all, read_to_string, remove_file};
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

const PUSH_CFG_PATH: &str = ".config/kafes/push";

fn setup_org() -> Result<()> {
    print("Setting up agent org...");
    let prev = load_global_config().ok();

    let gh_user = prompt_text("GitHub username", prev.as_ref().map(|p| p.gh_user.as_str()))?;
    if !valid_name(&gh_user) {
        bail!("invalid GitHub username");
    }
    gh_api(&format!("/users/{gh_user}")).context("user not found on GitHub")?;

    if prev.is_none() {
        let url = format!(
            "  https://github.com/account/organizations/new?plan=free  (e.g. {gh_user}-kafes)"
        );
        prompt_instruction(&[
            "Create a free organization you own, with no other members:",
            &url,
        ])?;
    }
    let default_owner = prev
        .as_ref()
        .map(|p| p.kafes_owner.clone())
        .unwrap_or(format!("{gh_user}-kafes"));
    let kafes_owner = prompt_text("kafes organization", Some(&default_owner))?;
    if !valid_name(&kafes_owner) || kafes_owner == gh_user {
        bail!("invalid kafes organization; it must not be your own account");
    }
    let owner = gh_api(&format!("/users/{kafes_owner}"))?;
    if owner["type"] != "Organization" {
        bail!("{kafes_owner} is not an organization");
    }

    let pem_dest = home_path(PUSH_CFG_PATH)?.join("app.pem");
    if !pem_dest.exists() {
        let url = format!("  https://github.com/organizations/{kafes_owner}/settings/apps/new");
        prompt_instruction(&[
            "Register a GitHub App owned by the organization:",
            &url,
            "  Contents: Read and write. Everything else: No access. Webhook: off",
            "  Installable: Only on this account.",
            "Then Private keys -> Generate a private key.",
        ])?;
        let pem = prompt_text("Path of the downloaded .pem", None)?;
        run_cmd("openssl", &["pkey", "-in", &pem, "-noout"]).context("not a private key")?;
        create_dir_all(home_path(PUSH_CFG_PATH)?)?;
        write_atomic(&pem_dest, &read_to_string(&pem_dest)?)?;
        remove_file(&pem)?;
        print_info(&format!(
            "key stored in {}; the download is deleted",
            pem_dest.display()
        ));
    }

    let client_id = prompt_text(
        "App Client ID (App settings, 'About')",
        prev.as_ref().map(|p| p.app.client_id.as_str()),
    )?;
    if !client_id.starts_with("Iv") {
        bail!("Client ID must start with 'Iv'");
    }

    let prev_slug = prev
        .as_ref()
        .map(|p| p.app.bot_name.trim_end_matches("[bot]"));
    let slug = prompt_text("App slug (last part of the App's settings URL)", prev_slug)?;
    let bot = gh_api(&format!("/users/{slug}/%5Bbot%5D")).context("no bot user; check the slug")?;
    let bot_name = bot["login"]
        .as_str()
        .context("bot has no login")?
        .to_string();
    let bot_id = bot["id"].as_u64().context("bot has no id")?;
    let bot_email = format!("{bot_id}+{bot_name}@users.noreply.github.com");
    print_info(&format!("kafes commit identity: {bot_name} <{bot_email}>"));

    save_global_config(&GlobalConfig {
        gh_user,
        kafes_owner,
        app: AppConfig {
            client_id,
            installation_id: prev.as_ref().and_then(|p| p.app.installation_id),
            bot_name,
            bot_email,
        },
        network: prev.and_then(|p| p.network),
    })
}

fn main() {
    println!("Hello, world!");
}
