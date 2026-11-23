use crate::ui::print_warning;
use anyhow::Result;
use std::sync::Mutex;

static HINT: Mutex<Option<String>> = Mutex::new(None);

fn print_hint() {
    if let Some(cmd) = HINT.lock().ok().and_then(|mut h| h.take()) {
        print_warning(&format!("stopped part-way; clean up with: {cmd}"))
    }
}

pub struct FailureHint;

impl FailureHint {
    pub fn arm(cleanup: &str) -> Self {
        if let Ok(mut h) = HINT.lock() {
            *h = Some(cleanup.to_string());
        }
        FailureHint
    }

    pub fn disarm(self) {
        if let Ok(mut h) = HINT.lock() {
            *h = None;
        }
    }
}

impl Drop for FailureHint {
    fn drop(&mut self) {
        print_hint();
    }
}

pub fn install_signal_handler() -> Result<()> {
    ctrlc::set_handler(|| {
        print_hint();
        std::process::exit(130);
    })?;
    Ok(())
}
