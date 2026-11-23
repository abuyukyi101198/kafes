use crate::constants::{SANDBOX_PATH, TASKS_PATH};
use crate::guard::Interactive;
use crate::ui::{print, print_info};
use crate::utils::home_path;
use anyhow::{Context, Result, bail, ensure};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

fn ws_cmd(task: &str) -> Result<Command> {
    let dir = home_path(TASKS_PATH)?.join(task);
    ensure!(dir.is_dir(), "{} is missing", dir.display());
    let mut cmd = Command::new("workshop");
    cmd.current_dir(dir).stdin(Stdio::null());
    Ok(cmd)
}

pub fn ws(task: &str, args: &[&str]) -> Result<()> {
    let status = ws_cmd(task)?
        .args(args)
        .status()
        .context("failed to run workshop")?;
    ensure!(status.success(), "workshop {} failed", args.join(" "));
    Ok(())
}

pub fn ws_output(task: &str, args: &[&str]) -> Result<String> {
    let out = ws_cmd(task)?
        .args(args)
        .output()
        .context("failed to run workshop")?;
    if !out.status.success() {
        bail!(
            "workshop {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?)
}

pub fn ws_exec_stdin(task: &str, script: &str, input: &str) -> Result<()> {
    let mut child = ws_cmd(task)?
        .args(["exec", "dev", "--", "sh", "-c", script])
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to run workshop")?;
    child
        .stdin
        .take()
        .context("no stdin")?
        .write_all(input.as_bytes())?;
    ensure!(child.wait()?.success(), "workshop exec failed: {script}");
    Ok(())
}

struct Conn {
    interface: String,
    plug: String,
    slot: String,
}

fn connections(task: &str) -> Result<Vec<Conn>> {
    let out = ws_output(task, &["connections", "dev"])?;
    let mut lines = out.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split_whitespace().collect();
    ensure!(
        header.starts_with(&["INTERFACE", "PLUG", "SLOT"]),
        "unexpected connections header"
    );

    let mut conns = Vec::new();
    for line in lines {
        let f: Vec<&str> = line.split_whitespace().collect();
        ensure!(f.len() >= 3, "unexpected connection line: {line}");
        if f[1] == "-" || f[2] == "-" {
            continue;
        }
        conns.push(Conn {
            interface: f[0].into(),
            plug: f[1].into(),
            slot: f[2].into(),
        });
    }

    Ok(conns)
}

pub fn cache_plugs(task: &str) -> Result<Vec<String>> {
    let mut plugs = Vec::new();
    for c in connections(task)? {
        let cache = c.plug.strip_prefix("dev/node:").is_some_and(|n| {
            !n.is_empty()
                && n.chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        });
        ensure!(
            c.interface == "mount" && cache && c.slot == "dev/system:mount",
            "unexpected connection: {} {} {}",
            c.interface,
            c.plug,
            c.slot
        );
        plugs.push(c.plug);
    }
    Ok(plugs)
}

pub fn connections_ok(task: &str) -> Result<()> {
    let (mut gw, mut push) = (0, 0);
    for c in connections(task)? {
        match (c.interface.as_str(), c.plug.as_str(), c.slot.as_str()) {
            ("tunnel", "dev/node:kafes-gateway", "dev/system:kafes-gateway") => gw += 1,
            ("tunnel", "dev/node:kafes-push", "dev/system:kafes-push") => push += 1,
            _ => bail!(
                "unexpected connection: {} {} {}",
                c.interface,
                c.plug,
                c.slot
            ),
        }
    }
    ensure!(
        gw == 1 && push == 1,
        "connections are not exactly the two tunnels"
    );
    Ok(())
}

pub fn reset_definition(task: &str) -> Result<()> {
    let dir = home_path(TASKS_PATH)?.join(task).join(".workshop");
    let meta = fs::symlink_metadata(&dir).with_context(|| format!("{} missing", dir.display()))?;
    ensure!(
        meta.is_dir(),
        "{} is not a plain directory; end the task instead",
        dir.display()
    );
    let dev = dir.join("dev.yaml");
    let _ = fs::remove_file(&dev);
    fs::copy(home_path(SANDBOX_PATH)?.join("dev.yaml"), &dev)?;
    Ok(())
}

pub fn open_shell(task: &str) -> Result<()> {
    print(&format!("Workshop terminal for {task}"));
    print_info("Run kafes-omp to start the agent; kafes-omp -c continues the last session.");
    print_info("Leave with exit. The workshop keeps running until kafes task stop or end.");
    let _interactive = Interactive::begin();
    let _ = ws_cmd(task)?
        .args(["shell", "dev"])
        .stdin(Stdio::inherit())
        .status();
    Ok(())
}
