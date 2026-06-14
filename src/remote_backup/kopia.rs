use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use tracing::{error, info};

use super::process::{replay_output, run_command_with_metrics};
use super::{
    RemoteBackupContext, RemoteBackupMetrics, RemoteBackupProvider, RemoteRestoreContext,
    RemoteSnapshotSummary,
};

pub struct KopiaRemoteBackupProvider<'a> {
    env_file: Option<&'a Path>,
}

impl<'a> KopiaRemoteBackupProvider<'a> {
    pub fn new(env_file: Option<&'a Path>) -> Self {
        Self { env_file }
    }

    fn command(&self) -> Result<Command> {
        let mut command = Command::new("kopia");
        apply_kopia_env(&mut command, self.env_file)?;
        Ok(command)
    }
}

impl RemoteBackupProvider for KopiaRemoteBackupProvider<'_> {
    fn name(&self) -> &'static str {
        "kopia"
    }

    fn validate_environment(&self) -> Result<()> {
        let output = self
            .command()?
            .arg("repository")
            .arg("status")
            .output()
            .context("Failed to run 'kopia repository status'")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!(
                "Kopia repository status failed. Return code: {:?}{}",
                output.status.code(),
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            );
        }

        Ok(())
    }

    fn backup_server(&self, context: RemoteBackupContext<'_>) -> Result<RemoteBackupMetrics> {
        self.validate_environment()?;
        info!(server = %context.server_name, source = %context.server_dir.display(), provider = self.name(), "starting remote backup");

        let mut command = self.command()?;
        command
            .arg("snapshot")
            .arg("create")
            .arg(context.server_dir)
            .arg("--tags")
            .arg("clserver:true")
            .arg("--tags")
            .arg(format!("server-id:{}", context.server_id))
            .arg("--tags")
            .arg(format!("server-name:{}", context.server_name));

        let output = run_command_with_metrics(&mut command)
            .context("Failed to run 'kopia snapshot create' for remote server backup")?;
        replay_output(&output)?;

        let metrics = RemoteBackupMetrics {
            duration: output.metrics.duration,
            user_cpu: output.metrics.user_cpu,
            system_cpu: output.metrics.system_cpu,
            peak_memory_kib: output.metrics.peak_memory_kib,
            bytes_added: None,
        };

        if !output.status.success() {
            error!(server = %context.server_name, exit_code = ?output.status.code(), provider = self.name(), duration_seconds = metrics.duration.as_secs_f64(), cpu_percent = metrics.cpu_utilization_percent(), peak_memory_kib = metrics.peak_memory_kib, "remote backup failed");
            bail!(
                "Remote backup failed for server '{}'. Return code: {:?}",
                context.server_name,
                output.status.code()
            );
        }

        info!(server = %context.server_name, provider = self.name(), duration_seconds = metrics.duration.as_secs_f64(), cpu_percent = metrics.cpu_utilization_percent(), user_cpu_seconds = metrics.user_cpu.map(|duration| duration.as_secs_f64()), system_cpu_seconds = metrics.system_cpu.map(|duration| duration.as_secs_f64()), peak_memory_kib = metrics.peak_memory_kib, "remote backup completed");
        Ok(metrics)
    }

    fn restore_snapshot(&self, context: RemoteRestoreContext<'_>) -> Result<()> {
        let restore_target = context.target.join(path_without_root(context.include));
        let snapshot = if context.snapshot == "latest" {
            self.latest_snapshot(context.server_id)?
                .and_then(|snapshot| snapshot.short_id)
                .ok_or_else(|| {
                    anyhow!(
                        "No Kopia snapshots found for server '{}'",
                        context.server_id
                    )
                })?
        } else {
            context.snapshot.to_string()
        };
        let snapshot_source = kopia_snapshot_source(&snapshot, context.include);

        info!(snapshot = %snapshot, source = %snapshot_source, target = %restore_target.display(), provider = self.name(), "starting remote restore staging");
        fs::create_dir_all(&restore_target).with_context(|| {
            format!(
                "Failed to create Kopia restore target directory '{}'",
                restore_target.display()
            )
        })?;

        let status = self
            .command()?
            .arg("snapshot")
            .arg("restore")
            .arg(&snapshot_source)
            .arg(&restore_target)
            .status()
            .context("Failed to run 'kopia snapshot restore' for remote server restore")?;

        if !status.success() {
            error!(snapshot = %snapshot, exit_code = ?status.code(), provider = self.name(), "remote restore staging failed");
            bail!(
                "Remote restore staging failed. Return code: {:?}",
                status.code()
            );
        }

        info!(snapshot = %snapshot, provider = self.name(), "remote restore staging completed");
        Ok(())
    }

    fn latest_snapshot(&self, server_id: &str) -> Result<Option<RemoteSnapshotSummary>> {
        self.validate_environment()?;
        let output = self
            .command()?
            .arg("snapshot")
            .arg("list")
            .arg("--tags")
            .arg(format!("server-id:{server_id}"))
            .output()
            .context("Failed to run 'kopia snapshot list' for backup status")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!(
                "Kopia snapshot status failed for server '{}'. Return code: {:?}{}",
                server_id,
                output.status.code(),
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_latest_kopia_snapshot_summary(&stdout))
    }

    fn print_snapshots(&self, server_id: &str, latest: usize) -> Result<()> {
        self.validate_environment()?;
        let mut command = self.command()?;
        command
            .arg("snapshot")
            .arg("list")
            .arg("--tags")
            .arg(format!("server-id:{server_id}"));
        if latest > 0 {
            command.arg("--max-results").arg(latest.to_string());
        }

        let status = command
            .status()
            .context("Failed to run 'kopia snapshot list' for backup snapshots")?;

        if !status.success() {
            bail!(
                "Kopia snapshot listing failed for server '{}'. Return code: {:?}",
                server_id,
                status.code()
            );
        }

        Ok(())
    }

    fn cleanup(&self, dry_run: bool) -> Result<()> {
        self.validate_environment()?;
        info!(
            dry_run,
            provider = self.name(),
            "starting remote backup maintenance"
        );

        let mut command = self.command()?;
        command.arg("maintenance").arg("run");
        if dry_run {
            command.arg("--full").arg("--dry-run");
        }

        let status = command
            .status()
            .context("Failed to run 'kopia maintenance run' for remote backup cleanup")?;

        if !status.success() {
            error!(exit_code = ?status.code(), dry_run, provider = self.name(), "remote backup maintenance failed");
            bail!(
                "Remote backup cleanup failed. Return code: {:?}",
                status.code()
            );
        }

        info!(
            dry_run,
            provider = self.name(),
            "remote backup maintenance completed"
        );
        Ok(())
    }
}

fn kopia_snapshot_source(snapshot: &str, include: &Path) -> String {
    if include.file_name().and_then(|name| name.to_str()) == Some("world") {
        format!("{snapshot}/world")
    } else {
        snapshot.to_string()
    }
}

fn parse_latest_kopia_snapshot_summary(output: &str) -> Option<RemoteSnapshotSummary> {
    output
        .lines()
        .rev()
        .find_map(parse_kopia_snapshot_summary_line)
}

fn parse_kopia_snapshot_summary_line(line: &str) -> Option<RemoteSnapshotSummary> {
    let snapshot_id = line
        .split_whitespace()
        .find(|part| part.len() >= 8 && part.chars().all(|ch| ch.is_ascii_hexdigit()))?;
    Some(RemoteSnapshotSummary {
        short_id: Some(snapshot_id.chars().take(8).collect()),
        time: line
            .split_whitespace()
            .find(|part| looks_like_date_time_prefix(part))
            .map(ToString::to_string),
    })
}

fn looks_like_date_time_prefix(value: &str) -> bool {
    value.len() >= 10
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
}

fn apply_kopia_env(command: &mut Command, env_file: Option<&Path>) -> Result<()> {
    for (key, value) in resolved_env_entries(env_file)? {
        command.env(key, value);
    }
    Ok(())
}

fn resolved_env_entries(env_file: Option<&Path>) -> Result<Vec<(String, String)>> {
    let mut entries = env_entries(env_file)?;
    if env_value(&entries, "KOPIA_PASSWORD").is_none()
        && let Some(password_file) = env_value(&entries, "KOPIA_PASSWORD_FILE")
    {
        let password = fs::read_to_string(&password_file)
            .with_context(|| format!("Failed to read KOPIA_PASSWORD_FILE '{}'", password_file))?;
        entries.push((
            "KOPIA_PASSWORD".to_string(),
            password.trim_end().to_string(),
        ));
    }

    Ok(entries)
}

fn env_entries(env_file: Option<&Path>) -> Result<Vec<(String, String)>> {
    match env_file {
        Some(env_file) => load_env_file(env_file),
        None => Ok(std::env::vars().collect()),
    }
}

fn env_value(entries: &[(String, String)], key: &str) -> Option<String> {
    entries
        .iter()
        .rev()
        .find_map(|(entry_key, value)| (entry_key == key).then(|| value.clone()))
}

fn load_env_file(path: &Path) -> Result<Vec<(String, String)>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Failed to read kopia env file '{}'", path.display()))?;

    text.lines()
        .enumerate()
        .filter_map(|(index, line)| parse_env_line(path, index + 1, line).transpose())
        .collect()
}

fn parse_env_line(path: &Path, line_number: usize, line: &str) -> Result<Option<(String, String)>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }

    let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
    let (key, value) = line.split_once('=').ok_or_else(|| {
        anyhow!(
            "Invalid env file line {} in '{}': expected KEY=value",
            line_number,
            path.display()
        )
    })?;
    let key = key.trim();

    if !is_valid_env_key(key) {
        bail!(
            "Invalid env variable name '{}' on line {} in '{}'",
            key,
            line_number,
            path.display()
        );
    }

    Ok(Some((key.to_string(), parse_env_value(value.trim()))))
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn parse_env_value(value: &str) -> String {
    let value = strip_inline_comment(value).trim();

    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
        {
            return value[1..value.len() - 1].to_string();
        }
    }

    value.to_string()
}

fn strip_inline_comment(value: &str) -> &str {
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;

    for (index, ch) in value.char_indices() {
        match ch {
            '\'' if !in_double_quotes => in_single_quotes = !in_single_quotes,
            '"' if !in_single_quotes => in_double_quotes = !in_double_quotes,
            '#' if !in_single_quotes && !in_double_quotes => return &value[..index],
            _ => {}
        }
    }

    value
}

fn path_without_root(path: &Path) -> PathBuf {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(PathBuf::from(part)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_world_snapshot_source() {
        assert_eq!(
            kopia_snapshot_source("abc123", Path::new("/srv/servers/survival/world")),
            "abc123/world"
        );
    }

    #[test]
    fn builds_all_snapshot_source() {
        assert_eq!(
            kopia_snapshot_source("abc123", Path::new("/srv/servers/survival")),
            "abc123"
        );
    }

    #[test]
    fn parses_kopia_env_lines() -> Result<()> {
        let path = Path::new("kopia.env");

        assert_eq!(
            parse_env_line(path, 1, "KOPIA_PASSWORD='secret'")?,
            Some(("KOPIA_PASSWORD".to_string(), "secret".to_string()))
        );
        assert_eq!(parse_env_line(path, 2, "# comment")?, None);
        Ok(())
    }

    #[test]
    fn resolves_kopia_password_file() -> Result<()> {
        let password_path = std::env::temp_dir().join(format!(
            "clserver-kopia-password-{}.pwd",
            std::process::id()
        ));
        fs::write(&password_path, "from-file\n")?;
        let env_path = std::env::temp_dir().join(format!(
            "clserver-kopia-password-file-{}.env",
            std::process::id()
        ));
        fs::write(
            &env_path,
            format!("KOPIA_PASSWORD_FILE='{}'\n", password_path.display()),
        )?;

        let entries = resolved_env_entries(Some(&env_path))?;

        assert_eq!(
            env_value(&entries, "KOPIA_PASSWORD").as_deref(),
            Some("from-file")
        );
        Ok(())
    }

    #[test]
    fn explicit_kopia_password_takes_precedence_over_password_file() -> Result<()> {
        let password_path = std::env::temp_dir().join(format!(
            "clserver-kopia-password-precedence-{}.pwd",
            std::process::id()
        ));
        fs::write(&password_path, "from-file\n")?;
        let env_path = std::env::temp_dir().join(format!(
            "clserver-kopia-password-precedence-{}.env",
            std::process::id()
        ));
        fs::write(
            &env_path,
            format!(
                "KOPIA_PASSWORD='explicit'\nKOPIA_PASSWORD_FILE='{}'\n",
                password_path.display()
            ),
        )?;

        let entries = resolved_env_entries(Some(&env_path))?;

        assert_eq!(
            env_value(&entries, "KOPIA_PASSWORD").as_deref(),
            Some("explicit")
        );
        Ok(())
    }
}
