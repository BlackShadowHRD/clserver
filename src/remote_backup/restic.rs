use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use tracing::{error, info};

use super::process::{replay_output, run_command_with_metrics};
use super::{
    RemoteBackupContext, RemoteBackupMetrics, RemoteBackupProvider, RemoteRestoreContext,
    RemoteSnapshotSummary,
};

pub struct ResticRemoteBackupProvider<'a> {
    env_file: Option<&'a Path>,
}

impl<'a> ResticRemoteBackupProvider<'a> {
    pub fn new(env_file: Option<&'a Path>) -> Self {
        Self { env_file }
    }

    fn command(&self) -> Result<Command> {
        let mut command = Command::new("restic");
        apply_restic_env(&mut command, self.env_file)?;
        Ok(command)
    }
}

impl RemoteBackupProvider for ResticRemoteBackupProvider<'_> {
    fn name(&self) -> &'static str {
        "restic"
    }

    fn validate_environment(&self) -> Result<()> {
        validate_restic_environment(self.env_file)
    }

    fn backup_server(&self, context: RemoteBackupContext<'_>) -> Result<RemoteBackupMetrics> {
        self.validate_environment()?;
        info!(server = %context.server_name, source = %context.server_dir.display(), provider = self.name(), "starting remote backup");

        let mut command = self.command()?;
        command
            .arg("backup")
            .arg(context.server_dir)
            .arg("--tag")
            .arg("clserver")
            .arg("--tag")
            .arg(format!("server-id:{}", context.server_id))
            .arg("--tag")
            .arg(format!("server-name:{}", context.server_name));

        let output = run_command_with_metrics(&mut command)
            .context("Failed to run 'restic backup' for remote server backup")?;
        replay_output(&output)?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut metrics = RemoteBackupMetrics {
            duration: output.metrics.duration,
            user_cpu: output.metrics.user_cpu,
            system_cpu: output.metrics.system_cpu,
            peak_memory_kib: output.metrics.peak_memory_kib,
            bytes_added: parse_restic_added_to_repository(&stdout)
                .or_else(|| parse_restic_added_to_repository(&stderr)),
        };

        if !output.status.success() {
            error!(server = %context.server_name, exit_code = ?output.status.code(), provider = self.name(), duration_seconds = metrics.duration.as_secs_f64(), cpu_percent = metrics.cpu_utilization_percent(), peak_memory_kib = metrics.peak_memory_kib, bytes_added = metrics.bytes_added, "remote backup failed");
            bail!(
                "Remote backup failed for server '{}'. Return code: {:?}",
                context.server_name,
                output.status.code()
            );
        }

        info!(server = %context.server_name, provider = self.name(), duration_seconds = metrics.duration.as_secs_f64(), cpu_percent = metrics.cpu_utilization_percent(), user_cpu_seconds = metrics.user_cpu.map(|duration| duration.as_secs_f64()), system_cpu_seconds = metrics.system_cpu.map(|duration| duration.as_secs_f64()), peak_memory_kib = metrics.peak_memory_kib, bytes_added = metrics.bytes_added, "remote backup completed");
        if metrics.bytes_added.is_none() {
            metrics.bytes_added = parse_restic_added_to_repository(&format!("{stdout}\n{stderr}"));
        }
        Ok(metrics)
    }

    fn restore_snapshot(&self, context: RemoteRestoreContext<'_>) -> Result<()> {
        info!(snapshot = context.snapshot, include = %context.include.display(), target = %context.target.display(), provider = self.name(), "starting remote restore staging");
        let status = self
            .command()?
            .arg("restore")
            .arg(context.snapshot)
            .arg("--target")
            .arg(context.target)
            .arg("--include")
            .arg(context.include)
            .status()
            .context("Failed to run 'restic restore' for remote server restore")?;

        if !status.success() {
            error!(snapshot = context.snapshot, exit_code = ?status.code(), provider = self.name(), "remote restore staging failed");
            bail!(
                "Remote restore staging failed. Return code: {:?}",
                status.code()
            );
        }

        info!(
            snapshot = context.snapshot,
            provider = self.name(),
            "remote restore staging completed"
        );
        Ok(())
    }

    fn latest_snapshot(&self, server_id: &str) -> Result<Option<RemoteSnapshotSummary>> {
        self.validate_environment()?;

        let output = self
            .command()?
            .arg("snapshots")
            .arg("--latest")
            .arg("1")
            .arg("--tag")
            .arg("clserver")
            .arg("--tag")
            .arg(format!("server-id:{server_id}"))
            .output()
            .context("Failed to run 'restic snapshots' for backup status")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!(
                "Restic snapshot status failed for server '{}'. Return code: {:?}{}",
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
        Ok(parse_latest_restic_snapshot_summary(&stdout))
    }

    fn print_snapshots(&self, server_id: &str, latest: usize) -> Result<()> {
        self.validate_environment()?;

        let mut command = self.command()?;
        command
            .arg("snapshots")
            .arg("--tag")
            .arg("clserver")
            .arg("--tag")
            .arg(format!("server-id:{server_id}"));
        if latest > 0 {
            command.arg("--latest").arg(latest.to_string());
        }

        let status = command
            .status()
            .context("Failed to run 'restic snapshots' for backup snapshots")?;

        if !status.success() {
            bail!(
                "Restic snapshot listing failed for server '{}'. Return code: {:?}",
                server_id,
                status.code()
            );
        }

        Ok(())
    }

    fn cleanup(&self, dry_run: bool) -> Result<()> {
        self.validate_environment()?;
        info!(
            keep_daily = 56,
            dry_run,
            provider = self.name(),
            "starting remote backup retention cleanup"
        );

        let mut command = self.command()?;
        command.arg("forget").arg("--keep-daily").arg("56");
        if dry_run {
            command.arg("--dry-run");
        } else {
            command.arg("--prune");
        }
        let status = command
            .status()
            .context("Failed to run 'restic forget' for remote backup cleanup")?;

        if !status.success() {
            error!(exit_code = ?status.code(), dry_run, provider = self.name(), "remote backup retention cleanup failed");
            bail!(
                "Remote backup cleanup failed. Return code: {:?}",
                status.code()
            );
        }

        if dry_run {
            info!(
                provider = self.name(),
                "remote backup retention cleanup dry run completed"
            );
        } else {
            info!(
                provider = self.name(),
                "remote backup retention cleanup completed"
            );
        }
        Ok(())
    }
}

fn parse_restic_added_to_repository(output: &str) -> Option<u64> {
    output.lines().find_map(|line| {
        let (_, value) = line.split_once("Added to the repository:")?;
        parse_restic_size(value.trim())
    })
}

fn parse_restic_size(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let amount = parts.next()?.replace(',', "");
    let unit = parts.next()?;
    let amount = amount.parse::<f64>().ok()?;
    let multiplier = match unit.trim_end_matches(',') {
        "B" => 1.0,
        "KiB" | "KB" => 1024.0,
        "MiB" | "MB" => 1024.0 * 1024.0,
        "GiB" | "GB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" | "TB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };

    Some((amount * multiplier).round() as u64)
}

fn parse_latest_restic_snapshot_summary(output: &str) -> Option<RemoteSnapshotSummary> {
    output.lines().find_map(parse_restic_snapshot_summary_line)
}

fn parse_restic_snapshot_summary_line(line: &str) -> Option<RemoteSnapshotSummary> {
    let mut parts = line.split_whitespace();
    let snapshot_id = parts.next()?;
    if !is_restic_snapshot_id(snapshot_id) {
        return None;
    }

    let date = parts.next()?;
    let time = parts.next()?;
    if !looks_like_restic_date(date) || !looks_like_restic_time(time) {
        return None;
    }

    Some(RemoteSnapshotSummary {
        short_id: Some(snapshot_id.to_string()),
        time: Some(format!("{date} {time}")),
    })
}

fn is_restic_snapshot_id(value: &str) -> bool {
    value.len() >= 8 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn looks_like_restic_date(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
}

fn looks_like_restic_time(value: &str) -> bool {
    value.len() >= 8
        && value.as_bytes().get(2) == Some(&b':')
        && value.as_bytes().get(5) == Some(&b':')
}

fn apply_restic_env(command: &mut Command, env_file: Option<&Path>) -> Result<()> {
    for (key, value) in restic_env_entries(env_file)? {
        command.env(key, value);
    }

    Ok(())
}

fn validate_restic_environment(env_file: Option<&Path>) -> Result<()> {
    let entries = restic_env_entries(env_file)?;

    if restic_env_value(&entries, "RESTIC_REPOSITORY").is_none() {
        bail!(
            "Restic repository is not configured. Set RESTIC_REPOSITORY{}.",
            env_file
                .map(|path| format!(" in '{}'", path.display()))
                .unwrap_or_else(|| " in the environment".to_string())
        );
    }

    if restic_env_value(&entries, "RESTIC_PASSWORD").is_none()
        && restic_env_value(&entries, "RESTIC_PASSWORD_FILE").is_none()
        && restic_env_value(&entries, "RESTIC_PASSWORD_COMMAND").is_none()
    {
        bail!(
            "Restic password is not configured. Set RESTIC_PASSWORD, RESTIC_PASSWORD_FILE, or RESTIC_PASSWORD_COMMAND{}.",
            env_file
                .map(|path| format!(" in '{}'", path.display()))
                .unwrap_or_else(|| " in the environment".to_string())
        );
    }

    Ok(())
}

fn restic_env_entries(env_file: Option<&Path>) -> Result<Vec<(String, String)>> {
    match env_file {
        Some(env_file) => load_env_file(env_file),
        None => Ok(std::env::vars().collect()),
    }
}

fn restic_env_value(entries: &[(String, String)], key: &str) -> Option<String> {
    entries
        .iter()
        .rev()
        .find_map(|(entry_key, value)| (entry_key == key).then(|| value.clone()))
}

fn load_env_file(path: &Path) -> Result<Vec<(String, String)>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Failed to read restic env file '{}'", path.display()))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_latest_restic_snapshot_summary_from_formatted_output() {
        let output = r#"
repository 12345678 opened (version 2)
ID        Time                 Host        Tags        Paths
------------------------------------------------------------------------
abcd1234  2026-06-10 03:04:05  host        clserver    /srv/servers/survival
------------------------------------------------------------------------
1 snapshots
"#;

        let summary = parse_latest_restic_snapshot_summary(output).expect("snapshot should parse");

        assert_eq!(summary.short_id.as_deref(), Some("abcd1234"));
        assert_eq!(summary.time.as_deref(), Some("2026-06-10 03:04:05"));
    }

    #[test]
    fn returns_none_when_formatted_restic_output_has_no_snapshots() {
        let output = r#"
repository 12345678 opened (version 2)
ID        Time                 Host        Tags        Paths
------------------------------------------------------------------------
0 snapshots
"#;

        assert!(parse_latest_restic_snapshot_summary(output).is_none());
    }

    #[test]
    fn parses_added_repository_size_from_restic_output() {
        let output = "Added to the repository: 12.5 MiB (10.1 MiB stored)";

        assert_eq!(
            parse_restic_added_to_repository(output),
            Some((12.5 * 1024.0 * 1024.0) as u64)
        );
    }

    #[test]
    fn parses_added_repository_size_with_commas() {
        let output = "Added to the repository: 1,024 KiB";

        assert_eq!(parse_restic_added_to_repository(output), Some(1024 * 1024));
    }

    #[test]
    fn parses_restic_env_file_lines() -> Result<()> {
        let path = Path::new("restic.env");

        assert_eq!(
            parse_env_line(path, 1, "RESTIC_REPOSITORY='s3:s3.example.com/bucket'")?,
            Some((
                "RESTIC_REPOSITORY".to_string(),
                "s3:s3.example.com/bucket".to_string()
            ))
        );
        assert_eq!(parse_env_line(path, 2, "# comment")?, None);
        Ok(())
    }

    #[test]
    fn strips_unquoted_inline_comments_from_env_values() {
        assert_eq!(parse_env_value("secret # comment"), "secret");
        assert_eq!(
            parse_env_value("'secret # not comment'"),
            "secret # not comment"
        );
    }

    #[test]
    fn validates_restic_environment_entries() -> Result<()> {
        let path =
            std::env::temp_dir().join(format!("clserver-restic-env-{}.env", std::process::id()));
        fs::write(
            &path,
            "RESTIC_REPOSITORY='s3:s3.example.com/bucket'\nRESTIC_PASSWORD_FILE='/secure/restic.pwd'",
        )?;

        validate_restic_environment(Some(&path))
    }
}
