mod cli;
mod config;
pub mod logging;
mod paths;
mod rcon;
mod server;

use anyhow::Result;

pub fn run() -> Result<()> {
    let request = cli::parse_request()?;

    if let cli::Action::Completions { shell } = request.action {
        cli::generate_completions(shell);
        return Ok(());
    }

    let _log_guard = logging::init("clserver", request.verbose)?;

    if request.verbose {
        tracing::debug!("verbose logging enabled");
    }

    let result = run_with_request(request);
    if let Err(err) = &result {
        tracing::error!(error = %format!("{err:#}"), "command failed");
    }

    result
}

fn run_with_request(request: cli::Request) -> Result<()> {
    let config = config::load_config()?;
    server::dispatch_request(request, config)
}
