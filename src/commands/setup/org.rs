use crate::config::{AppConfig, GlobalConfig, load_global_config, save_global_config, valid_name};
use crate::constants::PUSH_CFG_PATH;
use crate::github::gh_api;
use crate::ui::{print, print_info, prompt_instruction, prompt_text};
use crate::utils::{home_path, run_cmd, write_atomic};
use anyhow::{Context, Result, bail, ensure};
use std::fs::{create_dir_all, read_dir, read_to_string, remove_file};
use std::path::Path;

fn valid_client_id(id: &str) -> bool {
    id.strip_prefix("Iv").is_some_and(|rest| {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
    })
}

fn newest_download_pem() -> Option<String> {
    read_dir(home_path("Downloads").ok()?)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .ends_with(".private-key.pem")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path.to_string_lossy().into_owned())
}

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
        let o = &kafes_owner;
        prompt_instruction(&[
            &format!("Register a GitHub App owned by {o}:"),
            &format!("  https://github.com/organizations/{o}/settings/apps/new"),
            &format!("  Name: {o} (allowed: it's the owner's own name; the bot becomes {o}[bot])."),
            &format!(
                "    If taken, any unique name up to 34 characters.  Homepage URL: https://github.com/{o}"
            ),
            "  Callback URL, Setup URL, Webhook URL and secret: blank.",
            "  'Expire user authorization tokens': leave checked.",
            "  Unchecked: 'Request user authorization (OAuth) during installation',",
            "    'Enable Device Flow', 'Redirect on update', Webhook 'Active'.",
            "  Repository permissions: Contents = Read and write; Metadata = Read-only;",
            "    everything else, including Workflows and Pull requests, No access.",
            "  Organization and account permissions: none.",
            "  Where can this GitHub App be installed: Only on this account.",
            "Then on the App page, Private keys -> Generate a private key.",
            "  Leave the downloaded .pem where the browser saved it.",
        ])?;
        let newest = newest_download_pem();
        let pem = prompt_text("Path of the downloaded .pem", newest.as_deref())?;
        ensure!(Path::new(&pem).is_file(), "{pem} not found");
        run_cmd("openssl", &["pkey", "-in", &pem, "-noout"])
            .with_context(|| format!("{pem} is not a private key"))?;
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
    ensure!(valid_client_id(&client_id), "a Client ID starts with Iv");

    let prev_slug = prev
        .as_ref()
        .map(|p| p.app.bot_name.trim_end_matches("[bot]"));
    let slug = prompt_text("App slug (last part of the App's settings URL)", prev_slug)?;
    ensure!(valid_name(&slug), "invalid App slug: {slug}");
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
