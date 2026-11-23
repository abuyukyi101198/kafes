use crate::config::{NetworkConfig, load_global_config, save_global_config};
use crate::consts::{ACL, ACL_RULES};
use crate::ui::{print, print_info, prompt_text, step};
use crate::utils::{run_cmd, run_status};
use anyhow::{Context, Result};

pub fn setup_acl() -> Result<()> {
    print("Blocking the workshop from the host network...");
    // Cache sudo credentials so the captured `sudo lxc` calls below don't prompt.
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

    run_cmd("sudo", &["lxc", "network", "show", &bridge])
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

    print_info(&format!(
        "{ACL} attached to {bridge} (previous ACLs: '{acls_before}')"
    ));
    global.network = Some(NetworkConfig {
        bridge,
        acls_before,
    });
    save_global_config(&global)
}
