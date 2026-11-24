use crate::config::GlobalConfig;
use crate::constants::ACL;
use crate::ui;
use crate::utils::{run_cmd, run_status};
use crate::workshop::{connections_ok, ws_output};
use anyhow::{Context, Result, bail, ensure};
use std::net::Ipv4Addr;

pub struct Subnet {
    net: u32,
    mask: u32,
    cidr: String,
}

pub fn bridge_state(global: &GlobalConfig) -> Result<Subnet> {
    let bridge = &global
        .network
        .as_ref()
        .context("no bridge configured; run kafes init again")?
        .bridge;
    ui::info(&format!(
        "checking {ACL} on {bridge}; sudo may ask for your password"
    ));
    run_status("sudo", &["-v"])
        .context("sudo is needed to read the LXD bridge; nothing was started")?;

    let acls = run_cmd("sudo", &["lxc", "network", "get", bridge, "security.acls"])?;
    ensure!(
        acls.trim().split(',').any(|a| a == ACL),
        "{ACL} is no longer attached to {bridge}; run kafes init again"
    );

    let cidr = run_cmd("sudo", &["lxc", "network", "get", bridge, "ipv4.address"])?
        .trim()
        .to_string();
    let (addr, bits) = cidr
        .split_once('/')
        .with_context(|| format!("bad subnet {cidr}"))?;
    let addr: Ipv4Addr = addr.parse().with_context(|| format!("bad subnet {cidr}"))?;
    let bits: u32 = bits
        .parse()
        .ok()
        .filter(|b| *b <= 32)
        .with_context(|| format!("bad subnet {cidr}"))?;
    let mask = if bits == 0 {
        0
    } else {
        u32::MAX << (32 - bits)
    };
    Ok(Subnet {
        net: u32::from(addr) & mask,
        mask,
        cidr,
    })
}

pub fn acl_ok(task: &str, subnet: &Subnet) -> Result<()> {
    let out = ws_output(task, &["exec", "dev", "--", "hostname", "-I"])?;
    let on_bridge = out
        .split_whitespace()
        .filter_map(|a| a.parse::<Ipv4Addr>().ok())
        .any(|ip| u32::from(ip) & subnet.mask == subnet.net);
    if !on_bridge {
        bail!(
            "the workshop is not on the ACL bridge ({}); fix network.bridge in config.toml and run kafes init again",
            subnet.cidr
        );
    }
    Ok(())
}

pub fn check_workshop_network(task: &str, subnet: &Subnet) -> Result<()> {
    connections_ok(task)?;
    acl_ok(task, subnet)?;
    ui::done(&format!(
        "only the two tunnels are connected; {ACL} applies"
    ));
    Ok(())
}
