pub mod config;
pub mod utils;

use crate::config::{
    AppConfig, GlobalConfig, NetworkConfig, RepoConfig, load_global_config, load_repo_config,
    save_global_config, save_repo_config, valid_name,
};
use crate::utils::{gh_api, prompt_instruction, prompt_text, write_atomic};
use anyhow::{Context, Result, bail};
use std::fs::{create_dir_all, read_to_string, remove_file};
use std::path::{Path, PathBuf};
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

fn add_repo() -> Result<()> {
    let mut global = load_global_config()?;
    let name = prompt_text("Repository name", None)?;
    if !valid_name(&name) {
        bail!("invalid repository name");
    }
    print(&format!("Adding repository {name}"));
    let prev = load_repo_config(&name).ok();

    let default_checkout = prev
        .as_ref()
        .map(|p| p.checkout.to_string_lossy().into_owned());
    let checkout = prompt_text(
        &format!(
            "Absolute path of your local checkout of {}/{name}",
            global.gh_user
        ),
        default_checkout.as_deref(),
    )?;
    let checkout = PathBuf::from(checkout);
    if !checkout.is_absolute() {
        bail!("checkout path must be absolute");
    }

    let repo = RepoConfig {
        name,
        checkout,
        default_branch_before: prev.and_then(|p| p.default_branch_before),
    };
    let fork = repo.fork(&global);
    let dir = repo.checkout.to_string_lossy();

    run_cmd("git", &["-C", &dir, "rev-parse", "--git-dir"])
        .with_context(|| format!("{dir} is not a Git checkout"))?;
    let origin = run_cmd("git", &["-C", &dir, "remote", "get-url", "origin"])?;
    let origin = origin.trim().trim_end_matches('/').trim_end_matches(".git");
    if !(origin.ends_with(&format!("/{fork}")) || origin.ends_with(&format!(":{fork}"))) {
        bail!("origin of {dir} is {origin}, not {fork}");
    }
    gh_api(&format!("/repos/{fork}")).with_context(|| format!("{fork} not found or not public"))?;

    let kafes_fork = repo.kafes_fork(&global);
    if gh_api(&format!("/repos/{kafes_fork}")).is_err() {
        let url = format!("  https://github.com/{fork}/fork");
        let owner = format!(
            "  Owner: {}. Repository name: {} (keep it).",
            global.kafes_owner, repo.name
        );
        prompt_instruction(&[
            "Fork YOUR fork (not upstream) into the kafes organization:",
            &url,
            &owner,
        ])?;
    }
    let info = gh_api(&format!("/repos/{kafes_fork}")).context("create the kafes fork first")?;
    let parent = info["parent"]["full_name"].as_str().unwrap_or("nothing");
    if parent != fork {
        bail!("{kafes_fork} is a fork of {parent}, not of {fork}");
    }
    let url = format!("  https://github.com/{kafes_fork}/settings/actions");
    prompt_instruction(&[
        "Disable Actions on the kafes fork:",
        &url,
        "  Actions permissions: 'Disable actions'. Save.",
    ])?;

    let owner = &global.kafes_owner;
    match global.app.installation_id {
        None => {
            let line = format!("  Repository access: Only select repositories -> {kafes_fork}");
            prompt_instruction(&[
                &format!("Install the App: App settings -> Install App -> Install on {owner}."),
                &line,
                "The browser then shows .../settings/installations/<number>.",
            ])?;
            let id = prompt_text("Installation ID", None)?;
            global.app.installation_id =
                Some(id.parse().context("the installation ID must be a number")?);
            save_global_config(&global)?;
        }
        Some(id) => {
            let url =
                format!("  https://github.com/organizations/{owner}/settings/installations/{id}");
            let line = format!("  Repository access: add {kafes_fork}. Save.");
            prompt_instruction(&["Give the App access to the new fork:", &url, &line])?;
        }
    }

    save_repo_config(&repo)?;
    print_info(&format!("{} added", repo.name));
    Ok(())
}

fn setup_org() -> Result<()> {
    print("Setting up kafes org...");
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
        write_atomic(&pem_dest, &read_to_string(&pem)?)?;
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
    let bot = gh_api(&format!("/users/{slug}%5Bbot%5D")).context("no bot user; check the slug")?;
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

const ACL: &str = "kafes-no-lan";
const ACL_RULES: [&[&str]; 4] = [
    &[
        "egress",
        "action=reject",
        "destination=10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16,100.64.0.0/10",
    ],
    &["egress", "action=reject", "destination=fc00::/7,fe80::/10"],
    &["egress", "action=allow"],
    &["ingress", "action=allow"],
];

fn setup_acl() -> Result<()> {
    print("Blocking the workshop from the host network...");
    run_status("sudo", &["-v"])?;
    let mut global = load_global_config()?;

    let bridge = match global.network.as_ref() {
        Some(n) => n.bridge.clone(),
        None => {
            let csv = run_cmd("sudo", &["lxc", "network", "list", "--format", "csv"])?;
            let bridges: Vec<String> = csv
                .lines()
                .filter_map(|l| {
                    let mut cols = l.split(',');
                    let name = cols.next()?;
                    (cols.next()? == "bridge").then(|| name.to_string())
                })
                .collect();
            print_info(&format!("LXD bridges: {}", bridges.join(", ")));
            prompt_text("Workshop container bridge", Some("workshopbr0"))?
        }
    };

    run_cmd("sudo", &["lxc", "network", "acl", "show", &bridge])
        .with_context(|| format!("no LXD network {bridge}"))?;

    if run_cmd("sudo", &["lxc", "network", "acl", "show", ACL]).is_err() {
        step(ACL, || {
            run_cmd("sudo", &["lxc", "network", "acl", "create", ACL])?;
            for rule in ACL_RULES {
                let mut args = vec!["lxc", "network", "acl", "rule", "add", ACL];
                args.extend_from_slice(rule);
                run_cmd("sudo", &args)?;
            }
            Ok("created".to_string())
        })?;
    }

    let current = run_cmd("sudo", &["lxc", "network", "get", &bridge, "security.acls"])?;
    let current = current.trim();
    let mut acls_before = global
        .network
        .as_ref()
        .map(|n| n.acls_before.clone())
        .unwrap_or_default();

    if !current.split(',').any(|a| a == ACL) {
        acls_before = current.to_string();
        let value = if current.is_empty() {
            ACL.to_string()
        } else {
            format!("{current},{ACL}")
        };
        run_cmd(
            "sudo",
            &[
                "lxc",
                "network",
                "set",
                &bridge,
                &format!("security.acls={value}"),
            ],
        )?;
    }

    print_info(&format!("{ACL} attached to {bridge} (previous ACLs: '{acls_before}'"));
    global.network = Some(NetworkConfig {bridge, acls_before});
    save_global_config(&global)
}

fn main() {
    println!("Hello, world!");
}
