use anyhow::{Result, anyhow};
use console::strip_ansi_codes;

use crate::multiplexer::{create_backend, detect_backend};
use crate::workflow;

pub fn run(name: &str, lines: u16) -> Result<()> {
    let mux = create_backend(detect_backend());
    let (_path, agent) = workflow::resolve_worktree_agent(name, mux.as_ref())?;

    let output = mux
        .capture_pane(&agent.pane_id, lines)
        .ok_or_else(|| anyhow!("Failed to capture pane output"))?;

    // Strip ANSI escape codes
    let stripped = strip_ansi_codes(&output);

    // Remove blank lines after stripping ANSI, including whitespace-only rows.
    // Keep the CLI line limit as a final output guard.
    let trimmed: Vec<&str> = stripped
        .lines()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .skip_while(|l| l.trim().is_empty())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let start = trimmed.len().saturating_sub(lines as usize);
    for line in &trimmed[start..] {
        println!("{line}");
    }

    Ok(())
}
