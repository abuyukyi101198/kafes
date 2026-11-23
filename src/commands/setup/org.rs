use crate::config::{AppConfig, GlobalConfig, load_global_config, save_global_config, valid_name};
use crate::consts::PUSH_CFG_PATH;
use crate::github::gh_api;
use crate::ui::{print, print_info, prompt_instruction, prompt_text};
use crate::utils::{home_path, run_cmd, write_atomic};
use anyhow::{Context, Result, bail};
use std::fs::{create_dir_all, read_to_string, remove_file};

pub fn setup_org() -> Result<()> {
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
