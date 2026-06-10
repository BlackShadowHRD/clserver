use anyhow::{Result, anyhow};
use std::path::PathBuf;

pub fn config_dir() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|p| p.join("cls"))
        .ok_or_else(|| anyhow!("Failed to get config directory"))
}

pub fn state_dir() -> Result<PathBuf> {
    dirs::state_dir()
        .map(|p| p.join("cls"))
        .ok_or_else(|| anyhow!("Failed to get state directory"))
}

pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("cls.toml"))
}

pub fn log_file(program_name: &str) -> Result<PathBuf> {
    Ok(state_dir()?.join(format!("{program_name}.log")))
}
