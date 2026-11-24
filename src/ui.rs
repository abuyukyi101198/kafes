//! Terminal output, prompts and spinners
//!
//! Levels, from loud to quiet: `section` opens a stage, `done` reports a result, `info` says
//! something the user should read, `detail` is execution detail shown only with --verbose,
//! `warn` needs attention. --quiet keeps only warnings, content the user acts on (manual steps,
//! summaries before a prompt) and prompts. Messages go to stderr; stdout only carries a
//! command's data (the list tables), so it can be piped

use anyhow::{Result, bail};
use colored::{ColoredString, Colorize};
use indicatif::{ProgressBar, ProgressStyle};
use inquire::ui::{Color, RenderConfig, StyleSheet, Styled};
use inquire::{Confirm, MultiSelect, Select, Text, required};
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum Verbosity {
    Quiet = 0,
    Brief = 1,
    Verbose = 2,
}

static VERBOSITY: AtomicU8 = AtomicU8::new(Verbosity::Brief as u8);
static INTERACTIVE: AtomicBool = AtomicBool::new(true);

fn shown(level: Verbosity) -> bool {
    VERBOSITY.load(Ordering::Relaxed) >= level as u8
}

pub fn init(no_color: bool, no_input: bool, verbosity: Verbosity) {
    let color = !no_color
        && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
        && std::env::var("TERM").map_or(true, |t| t != "dumb")
        && std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal();
    colored::control::set_override(color);
    inquire::set_global_render_config(if color {
        render_config()
    } else {
        RenderConfig::empty()
    });
    INTERACTIVE.store(
        !no_input && std::io::stdin().is_terminal(),
        Ordering::Relaxed,
    );
    VERBOSITY.store(verbosity as u8, Ordering::Relaxed);
}

pub fn require_interactive(what: &str) -> Result<()> {
    if !interactive() {
        bail!("cannot {what}: input is not interactive (--no-input, or stdin is not a terminal)");
    }
    Ok(())
}

pub fn interactive() -> bool {
    INTERACTIVE.load(Ordering::Relaxed)
}

fn render_config() -> RenderConfig<'static> {
    let grey = StyleSheet::new().with_fg(Color::DarkGrey);
    RenderConfig::default_colored()
        .with_prompt_prefix(Styled::new("?").with_fg(Color::LightCyan))
        .with_answered_prompt_prefix(Styled::new("✔").with_fg(Color::LightGreen))
        .with_answer(StyleSheet::new().with_fg(Color::LightCyan))
        .with_default_value(grey)
        .with_help_message(grey)
}

pub fn cmd(s: &str) -> String {
    s.cyan().to_string()
}

pub fn url(s: &str) -> String {
    s.blue().underline().to_string()
}

pub fn em(s: &str) -> String {
    s.bold().to_string()
}

pub fn section(title: &str) {
    if shown(Verbosity::Brief) {
        eprintln!("\n{} {}", "▸".cyan().bold(), title.bold());
    }
}

pub fn done(msg: &str) {
    if shown(Verbosity::Brief) {
        eprintln!("  {} {msg}", "✔".green());
    }
}

pub fn info(msg: &str) {
    if shown(Verbosity::Brief) {
        eprintln!("  {msg}");
    }
}

pub fn detail(msg: &str) {
    if shown(Verbosity::Verbose) {
        eprintln!("  {}", msg.dimmed());
    }
}

pub fn warn(msg: &str) {
    eprintln!("  {} {msg}", "warning:".yellow().bold());
}

pub fn fields(rows: &[(&str, String)]) {
    let w = rows
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    for (k, v) in rows {
        emit(&format!("  {}  {v}", format!("{k:<w$}").dimmed()));
    }
}

pub fn list(title: &str, items: &[String]) {
    if items.is_empty() {
        emit(&format!(
            "  {} {}",
            format!("{title}:").dimmed(),
            "none".dimmed()
        ));
        return;
    }
    emit(&format!("  {}", format!("{title}:").dimmed()));
    for item in items {
        emit(&format!("    • {item}"));
    }
}

static DATA_TO_STDOUT: AtomicBool = AtomicBool::new(false);

pub fn data_to_stdout() {
    DATA_TO_STDOUT.store(true, Ordering::Relaxed);
}

fn emit(line: &str) {
    if DATA_TO_STDOUT.load(Ordering::Relaxed) {
        println!("{line}");
    } else {
        eprintln!("{line}");
    }
}

pub fn next(steps: &[(String, &str)]) {
    if !shown(Verbosity::Brief) {
        return;
    }
    eprintln!("\n{}", "Next:".bold());
    let w = steps
        .iter()
        .map(|(c, _)| c.chars().count())
        .max()
        .unwrap_or(0);
    for (c, what) in steps {
        eprintln!("  {}  {}", format!("{c:<w$}").cyan(), what.dimmed());
    }
}

#[derive(Clone, Copy)]
pub enum Tone {
    Plain,
    Bold,
    Good,
    Caution,
    Bad,
}

pub fn table(headers: &[&str], rows: &[Vec<(String, Tone)>], no_headers: bool) {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, (cell, _)) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let last = headers.len() - 1;
    // Padded before styling, so escape codes do not skew the columns
    let pad = |i: usize, s: &str| {
        if i == last {
            s.to_string()
        } else {
            format!("{s:<w$}", w = widths[i])
        }
    };
    if !no_headers {
        let line: Vec<String> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| pad(i, &h.to_uppercase()).bold().to_string())
            .collect();
        println!("{}", line.join("  ").trim_end());
    }
    for row in rows {
        let line: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, (cell, tone))| {
                let s = pad(i, cell);
                let styled: ColoredString = match tone {
                    Tone::Plain => s.normal(),
                    Tone::Bold => s.bold(),
                    Tone::Good => s.green(),
                    Tone::Caution => s.yellow(),
                    Tone::Bad => s.red(),
                };
                styled.to_string()
            })
            .collect();
        println!("{}", line.join("  ").trim_end());
    }
}

pub fn diff(text: &str) {
    for line in text.lines() {
        let styled = match line.chars().next() {
            _ if line.starts_with("+++") || line.starts_with("---") => line.bold(),
            Some('+') => line.green(),
            Some('-') => line.red(),
            Some('@') => line.cyan(),
            _ => line.dimmed(),
        };
        eprintln!("    {styled}");
    }
}

enum Item {
    Step(String),
    Text(String),
    Link(String),
    Cmd(String),
    Field(String, String),
}

pub struct Manual {
    title: String,
    items: Vec<Item>,
}

impl Manual {
    pub fn new(title: impl Into<String>) -> Self {
        Manual {
            title: title.into(),
            items: Vec::new(),
        }
    }

    pub fn step(mut self, s: impl Into<String>) -> Self {
        self.items.push(Item::Step(s.into()));
        self
    }

    pub fn text(mut self, s: impl Into<String>) -> Self {
        self.items.push(Item::Text(s.into()));
        self
    }

    pub fn link(mut self, s: impl Into<String>) -> Self {
        self.items.push(Item::Link(s.into()));
        self
    }

    pub fn cmd(mut self, s: impl Into<String>) -> Self {
        self.items.push(Item::Cmd(s.into()));
        self
    }

    pub fn field(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.items.push(Item::Field(k.into(), v.into()));
        self
    }

    pub fn print(&self) {
        eprintln!(
            "\n{} {}",
            " TO DO ".black().on_cyan().bold(),
            self.title.bold()
        );
        let has_steps = self.items.iter().any(|i| matches!(i, Item::Step(_)));
        let pad = if has_steps { "     " } else { "  " };
        let w = self
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Field(k, _) => Some(k.chars().count()),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let mut n = 0;
        let mut prev_cmd = false;
        for item in &self.items {
            let is_cmd = matches!(item, Item::Cmd(_));
            if is_cmd && !prev_cmd {
                eprintln!();
            }
            if !is_cmd && prev_cmd {
                eprintln!();
            }
            prev_cmd = is_cmd;
            match item {
                Item::Step(s) => {
                    n += 1;
                    eprintln!("  {} {s}", format!("{n:>2}.").cyan().bold());
                }
                Item::Text(s) => eprintln!("{pad}{}", s.dimmed()),
                Item::Link(s) => eprintln!("{pad}{}", url(s)),
                Item::Cmd(s) => eprintln!("{pad}{}", s.cyan()),
                Item::Field(k, v) => {
                    eprintln!("{pad}{}  {}", format!("{k:<w$}").dimmed(), v.bold())
                }
            }
        }
        if prev_cmd {
            eprintln!();
        }
    }

    pub fn confirm(self) -> Result<()> {
        require_interactive(&format!("wait for the manual step '{}'", self.title))?;
        self.print();
        if !Confirm::new("Done?").with_default(true).prompt()? {
            bail!("stopped; run the same command again once this step is done")
        }
        Ok(())
    }
}

fn label(msg: &str) -> String {
    if msg.ends_with(['?', ':']) {
        msg.to_string()
    } else {
        format!("{msg}:")
    }
}

fn ask(msg: &str) -> Result<()> {
    require_interactive(&format!("ask \"{msg}\""))
}

pub fn confirm(msg: &str) -> Result<bool> {
    ask(msg)?;
    Ok(Confirm::new(msg).with_default(true).prompt()?)
}

pub fn confirm_help(msg: &str, help: &str) -> Result<bool> {
    ask(msg)?;
    Ok(Confirm::new(msg)
        .with_default(true)
        .with_help_message(help)
        .prompt()?)
}

pub fn confirm_danger(msg: &str, help: Option<&str>) -> Result<bool> {
    ask(msg)?;
    let mut prompt = Confirm::new(msg).with_default(false);
    if let Some(h) = help {
        prompt = prompt.with_help_message(h);
    }
    Ok(prompt.prompt()?)
}

pub fn multiselect(msg: &str, options: Vec<String>) -> Result<Vec<String>> {
    ask(msg)?;
    Ok(MultiSelect::new(&label(msg), options).prompt()?)
}

pub fn select(msg: &str, options: Vec<String>) -> Result<String> {
    ask(msg)?;
    Ok(Select::new(&label(msg), options).prompt()?)
}

pub fn text(msg: &str, default: Option<&str>) -> Result<String> {
    text_help(msg, default, None)
}

pub fn text_help(msg: &str, default: Option<&str>, help: Option<&str>) -> Result<String> {
    ask(msg)?;
    let msg = label(msg);
    let mut prompt = Text::new(&msg).with_validator(required!());
    if let Some(d) = default.filter(|d| !d.is_empty()) {
        prompt = prompt.with_default(d);
    }
    if let Some(h) = help {
        prompt = prompt.with_help_message(h);
    }
    Ok(prompt.prompt()?.trim().to_string())
}

pub fn step<F>(label: &str, f: F) -> Result<String>
where
    F: FnOnce() -> Result<String>,
{
    if !shown(Verbosity::Brief) {
        return f();
    }
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("  {spinner:.cyan} {msg}")?);
    pb.set_message(label.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));

    let res = f();
    let msg = match &res {
        Ok(detail) => format!("{} {label} {}", "✔".green(), detail.dimmed()),
        Err(e) => format!("{} {label}: {e}", "✘".red()),
    };
    if pb.is_hidden() {
        eprintln!("  {msg}");
    } else {
        pb.set_style(ProgressStyle::with_template("  {msg}")?);
        pb.finish_with_message(msg);
    }
    res
}
