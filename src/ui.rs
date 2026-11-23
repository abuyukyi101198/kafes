//! Terminal output, prompts and spinners.

use anyhow::{Result, bail};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use inquire::{Confirm, MultiSelect, Text, required};
use std::time::Duration;

pub fn print(msg: &str) {
    println!("\n{}", msg.bold());
}

pub fn print_info(msg: &str) {
    println!("  {msg}");
}

pub fn print_warning(msg: &str) {
    eprintln!("{} {msg}", "WARNING".bold().yellow());
}

pub fn prompt_confirm(msg: &str) -> Result<bool> {
    Ok(Confirm::new(msg).with_default(true).prompt()?)
}

pub fn prompt_multiselect(msg: &str, options: Vec<String>) -> Result<Vec<String>> {
    Ok(MultiSelect::new(msg, options).prompt()?)
}

pub fn prompt_text(msg: &str, default: Option<&str>) -> Result<String> {
    let mut prompt = Text::new(msg).with_validator(required!());
    if let Some(d) = default.filter(|d| !d.is_empty()) {
        prompt = prompt.with_default(d);
    }
    Ok(prompt.prompt()?.trim().to_string())
}

pub fn prompt_instruction(lines: &[&str]) -> Result<()> {
    println!("\n{}", "[manual]".cyan());
    for line in lines {
        println!("      {line}");
    }
    if !prompt_confirm("Done?")? {
        bail!("stopped; re-run the same command once this step is done")
    }
    Ok(())
}

pub fn step<F>(label: &str, f: F) -> Result<String>
where
    F: FnOnce() -> Result<String>,
{
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("{spinner} {msg}")?);
    pb.set_message(label.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));

    let res = f();
    pb.set_style(ProgressStyle::with_template("{msg}")?);
    match &res {
        Ok(detail) => pb.finish_with_message(format!("{} {label} {detail}", "✔".green())),
        Err(e) => pb.finish_with_message(format!("{} {label}: {e}", "✘".red())),
    }
    res
}
