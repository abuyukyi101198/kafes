//! `kafes repo edit` and `kafes repo show`, and the profile choice in `repo add`

use crate::commands::setup::setup_check;
use crate::config::{load_global_config, load_repo_config};
use crate::profile::{
    Profile, check_path, draft_path, load_profile, load_profile_text, parse_profile, preset_names,
    preset_text, save_profile_text,
};
use crate::ui::{print, print_info, print_warning, prompt_confirm, prompt_select};
use crate::utils::{run_status, write_atomic};
use anyhow::{Context, Result, bail, ensure};
use std::fs;

/// Keeps an existing profile unless a preset is named; otherwise shows the
/// chosen preset in full and saves a copy once confirmed
pub fn choose_profile(name: &str, preset: Option<String>) -> Result<Profile> {
    let text = match (preset, load_profile_text(name)?) {
        (None, Some(text)) => {
            let profile = parse_profile(&text).with_context(|| format!("profile of {name}"))?;
            print_info(&format!(
                "using {name}'s saved profile; change it with: kafes repo edit {name}"
            ));
            return Ok(profile);
        }
        (Some(p), _) => preset_text(&p)?,
        (None, None) => preset_text(&prompt_select(
            &format!("Profile for {name}"),
            preset_names(),
        )?)?,
    };
    let profile = parse_profile(text)?;
    print(&format!("Profile for {name}"));
    profile.print_summary();
    ensure!(
        prompt_confirm("Use this profile?")?,
        "stopped; pick another with --profile"
    );
    save_profile_text(name, text)?;
    Ok(profile)
}

pub fn cmd_edit(name: &str, preset: Option<String>) -> Result<()> {
    let global = load_global_config()?;
    let mut repo = load_repo_config(name)?;
    match preset {
        Some(p) => {
            choose_profile(name, Some(p))?;
        }
        None => edit_in_editor(name)?,
    }
    // The rules may have changed: bring the fork's kafes-checks in line
    setup_check(&global, &mut repo)?;
    print(&format!("Profile of {name} saved"));
    print_info("prompt and rules: from the next task shell or start");
    print_info("toolchain and install: from the next task new");
    Ok(())
}

fn edit_in_editor(name: &str) -> Result<()> {
    let current = load_profile_text(name)?.with_context(|| {
        format!("{name} has no profile; set one with: kafes repo edit {name} --profile <preset>")
    })?;
    let draft = draft_path(name)?;
    write_atomic(&draft, &current)?;
    let result = edit_loop(name, &current, &draft.to_string_lossy());
    let _ = fs::remove_file(&draft);
    result
}

fn edit_loop(name: &str, current: &str, draft: &str) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let mut parts = editor.split_whitespace();
    let program = parts.next().context("$VISUAL or $EDITOR is empty")?;
    let mut args: Vec<&str> = parts.collect();
    args.push(draft);

    loop {
        run_status(program, &args)?;
        let text = fs::read_to_string(draft)?;
        if text == current {
            print_info("no changes");
            return Ok(());
        }
        match parse_profile(&text) {
            Ok(profile) => {
                print(&format!("Profile for {name}"));
                profile.print_summary();
                if !prompt_confirm("Save this profile?")? {
                    bail!("stopped; the profile is unchanged");
                }
                return save_profile_text(name, &text);
            }
            Err(e) => {
                print_warning(&format!("{e:#}"));
                ensure!(
                    prompt_confirm("Edit again?")?,
                    "stopped; the profile is unchanged"
                );
            }
        }
    }
}

pub fn cmd_show(name: &str) -> Result<()> {
    load_repo_config(name)?;
    let profile = load_profile(name)?;
    print(&format!("Profile for {name}"));
    profile.print_summary();
    print_info(&format!(
        "edit-rule workflow rendered from it: {}",
        check_path(name)?.display()
    ));
    Ok(())
}


