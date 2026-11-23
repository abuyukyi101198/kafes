use crate::constants::{
    BUN_VERSION, CHECK_YML, DEV_YAML, OMP_VERSION, POLICY, PROMPT, PROXY_TS, SANDBOX_PATH, SETUP_WS,
};
use crate::ui::{print, print_info, print_warning, prompt_confirm};
use crate::utils::{home_path, write_atomic_mode};
use anyhow::{Context, Result, bail};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn setup_proxy_files() -> Result<()> {
    print("Setting up trusted files and push proxy...");
    write_trusted_files()?;
    let kafes = install_self()?;
    link_command(&kafes)?;
    let sandbox = home_path(SANDBOX_PATH)?;
    print_info(&format!(
        "trusted files written to {} (no secrets)",
        sandbox.display()
    ));

    let proxy = sandbox.join("kafes-push-proxy.ts");
    if !prompt_confirm(&format!("Have you read {}?", proxy.display()))? {
        bail!("read the proxy before running it");
    }
    Ok(())
}

fn write_trusted_files() -> Result<()> {
    let sandbox = home_path(SANDBOX_PATH)?;
    fs::create_dir_all(&sandbox)?;

    let setup_ws = SETUP_WS
        .replace("{{BUN_VERSION}}", BUN_VERSION)
        .replace("{{OMP_VERSION}}", OMP_VERSION);

    let files: [(&str, &str, u32); 7] = [
        ("dev.yaml", DEV_YAML, 0o644),
        ("setup-workshop.sh", &setup_ws, 0o644),
        ("kafes-policy.yml", POLICY, 0o644),
        ("kafes-prompt.md", PROMPT, 0o644),
        (
            "bunfig.toml",
            "# Intentionally no Bun settings or preloads.\n",
            0o644,
        ),
        ("kafes-push-proxy.ts", PROXY_TS, 0o644),
        ("kafes-pr-check.yml", CHECK_YML, 0o644),
    ];
    for (name, text, mode) in files {
        write_atomic_mode(&sandbox.join(name), text, mode)?;
    }
    Ok(())
}

fn install_self() -> Result<PathBuf> {
    let dest = home_path(SANDBOX_PATH)?.join("kafes");
    let exe = std::env::current_exe()?.canonicalize()?;
    if exe != dest {
        let tmp = dest.with_extension("tmp");
        fs::copy(&exe, &tmp).context("can't copy the kafes binary")?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
        fs::rename(&tmp, &dest)?;
        print_info(&format!("copied kafes into {}", dest.display()));
    }
    Ok(dest)
}

fn link_command(target: &Path) -> Result<()> {
    let link = home_path(".local/bin/kafes")?;
    fs::create_dir_all(link.parent().context("bad link path")?)?;

    match fs::read_link(&link) {
        Ok(current) if current == target => {}
        Ok(_) => {
            print_warning(&format!(
                "{} exists and isn't kafes's link; left alone",
                link.display()
            ));
            return Ok(());
        }
        Err(_) if link.exists() => {
            print_warning(&format!(
                "{} exists and isn't a link; left alone",
                link.display()
            ));
            return Ok(());
        }
        Err(_) => {
            std::os::unix::fs::symlink(target, &link)?;
            print_info(&format!(
                "linked {} -> {}",
                link.display(),
                target.display()
            ));
        }
    }

    let bin_dir = home_path(".local/bin")?;
    let on_path =
        std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == bin_dir));
    if on_path {
        print_info("from now on, run it as: kafes");
    } else {
        print_warning(
            "~/.local/bin is not on PATH yet; add: export PATH=\"$HOME/.local/bin:$PATH\"",
        );
    }
    Ok(())
}
