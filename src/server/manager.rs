use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, SystemTime};

use crate::config::{BackupConfig, GlobalConfig, RestoreMode, ServerConfig, resolve_java_bin};
use crate::remote_backup::{
    RemoteBackupContext, RemoteBackupProvider, RemoteRestoreContext, format_remote_backup_metrics,
    remote_backup_provider,
};
use tracing::{debug, error, info, warn};

pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(900);
pub const DEFAULT_STOP_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct LocalBackupStatus {
    pub path: Option<PathBuf>,
    pub exists: bool,
    pub latest_modified: Option<SystemTime>,
}

pub struct RemoteRestorePlan {
    temp_dir: PathBuf,
    staged_source: PathBuf,
    destination: PathBuf,
    mode: RestoreMode,
}

pub struct ServerManager {
    pub server_id: String,
    pub config: ServerConfig,
    java_bin: String,
    server_dir: PathBuf,
    log_dir: PathBuf,
    local_backup_dir: Option<PathBuf>,
    backup_config: BackupConfig,
}

impl ServerManager {
    pub fn new(
        server_id: String,
        config: ServerConfig,
        global: &GlobalConfig,
        backup: &BackupConfig,
        java_environments: &HashMap<String, String>,
    ) -> Result<Self> {
        let java_bin = resolve_java_bin(&config, java_environments)?;
        let server_dir = global.server_dir.join(&config.name);
        let log_dir = global.log_dir.clone();
        let local_backup_dir = backup.local_dir.clone();
        let backup_config = backup.clone();

        Ok(Self {
            server_id,
            config,
            java_bin,
            server_dir,
            log_dir,
            local_backup_dir,
            backup_config,
        })
    }

    pub fn screen_session_exists(&self) -> Result<bool> {
        let output = Command::new("screen")
            .arg("-ls")
            .output()
            .context("Failed to run 'screen -ls'")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(screen_session_exists_in_output(&stdout, &self.config.name))
    }

    pub fn start_server(&self) -> Result<()> {
        if self.screen_session_exists()? {
            warn!(server = %self.config.name, "server is already running");
            return Ok(());
        }

        let command = if let Some(start_command) = &self.config.start_command {
            start_command.clone()
        } else {
            let java_params = self.config.java_params.as_deref().unwrap_or("");
            let jar_file = self.config.jar_file.as_deref().ok_or_else(|| {
                anyhow!(
                    "Server '{}' needs either startCommand or jarFile.",
                    self.config.name
                )
            })?;
            format!("{} {} -jar {}", self.java_bin, java_params, jar_file)
        };

        debug!(server = %self.config.name, command = %command, "generated start command");

        let timestamp = Local::now().format("%Y-%m-%d_%H:%M:%S");
        let log_file_screen = self
            .log_dir
            .join("servers")
            .join(&self.config.name)
            .join(format!("{timestamp}.log"));

        if let Some(parent) = log_file_screen.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create screen log directory '{}' for server '{}'",
                    parent.display(),
                    self.config.name
                )
            })?;
        }

        let status = Command::new("screen")
            .current_dir(&self.server_dir)
            .arg("-dmS")
            .arg(&self.config.name)
            .arg("-L")
            .arg("-Logfile")
            .arg(&log_file_screen)
            .arg("bash")
            .arg("-c")
            .arg(&command)
            .status()
            .with_context(|| {
                format!(
                    "Failed to start screen session in {}",
                    self.server_dir.display()
                )
            })?;

        if !status.success() {
            let message = format!(
                "Error starting server '{}'. Return code: {:?}",
                self.config.name,
                status.code()
            );
            error!(server = %self.config.name, exit_code = ?status.code(), "failed to start server");
            bail!(message);
        }

        info!(server = %self.config.name, "server started successfully");
        Ok(())
    }

    pub fn stop_with_stop_command(&self) -> Result<()> {
        if !self.screen_session_exists()? {
            warn!(server = %self.config.name, "server is not running");
            return Ok(());
        }

        let stop_command = self.config.stop_command.as_deref().ok_or_else(|| {
            anyhow!(
                "Server '{}' has no stopCommand configured. Minecraft servers should stop through RCON instead.",
                self.config.name
            )
        })?;

        let status = Command::new("screen")
            .args([
                "-S",
                &self.config.name,
                "-X",
                "stuff",
                &format!("{stop_command}\n"),
            ])
            .status()
            .context("Failed to send stop command to screen session")?;

        if !status.success() {
            let message = format!(
                "Error sending stop command to server '{}'. Return code: {:?}",
                self.config.name,
                status.code()
            );
            error!(server = %self.config.name, exit_code = ?status.code(), "failed to send stop command");
            bail!(message);
        }

        info!(server = %self.config.name, "server stopping");
        Ok(())
    }

    pub fn stop_with_stop_command_and_wait(&self) -> Result<()> {
        self.stop_with_stop_command()?;
        self.wait_until_stopped_or_timeout(DEFAULT_STOP_TIMEOUT, DEFAULT_STOP_POLL_INTERVAL)
    }

    pub fn restart_with_stop_command(&self) -> Result<()> {
        self.stop_with_stop_command_and_wait()?;
        self.start_server()
    }

    pub fn wait_until_stopped(&self, timeout: Duration, poll_interval: Duration) -> Result<bool> {
        let deadline = std::time::Instant::now() + timeout;

        while std::time::Instant::now() < deadline {
            if !self.screen_session_exists()? {
                info!(server = %self.config.name, "server is fully stopped");
                return Ok(true);
            }

            sleep(poll_interval);
        }

        warn!(server = %self.config.name, timeout_seconds = timeout.as_secs(), "timed out waiting for server to stop");
        Ok(false)
    }

    pub fn wait_until_stopped_or_timeout(
        &self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<()> {
        if self.wait_until_stopped(timeout, poll_interval)? {
            Ok(())
        } else {
            bail!(
                "Timed out waiting for server '{}' to stop",
                self.config.name
            );
        }
    }

    pub fn attach_server(&self) -> Result<()> {
        let status = Command::new("screen")
            .args(["-r", &self.config.name])
            .status()
            .context("Failed to attach screen session")?;

        if !status.success() {
            let message = format!(
                "Error attaching screen session '{}'. Return code: {:?}",
                self.config.name,
                status.code()
            );
            error!(server = %self.config.name, exit_code = ?status.code(), "failed to attach screen session");
            bail!(message);
        }

        info!(server = %self.config.name, "screen session attached");
        Ok(())
    }

    pub fn status_server(&self) -> Result<()> {
        let screen_running = self.screen_session_exists()?;
        let server_dir_status = path_status(&self.server_dir).with_context(|| {
            format!(
                "Failed to inspect server directory '{}'",
                self.server_dir.display()
            )
        })?;
        let screen_log_dir = self.log_dir.join("servers").join(&self.config.name);
        let latest_log = latest_file_in_directory(&screen_log_dir).with_context(|| {
            format!(
                "Failed to inspect screen log directory '{}'",
                screen_log_dir.display()
            )
        })?;

        println!("ID: {}", self.server_id);
        println!("Server: {}", self.config.name);
        println!("Type: {}", self.config.server_type);
        println!(
            "Screen session: {}",
            if screen_running {
                "running"
            } else {
                "not running"
            }
        );
        println!("Server directory: {}", self.server_dir.display());
        println!("Server directory status: {server_dir_status}");
        println!("Screen log directory: {}", screen_log_dir.display());

        if let Some(latest_log) = latest_log {
            println!("Latest screen log: {}", latest_log.display());
        } else {
            println!("Latest screen log: none found");
        }

        println!("Java executable: {}", self.java_bin);
        println!(
            "Start mode: {}",
            if self.config.start_command.is_some() {
                "custom startCommand"
            } else {
                "generated Java command"
            }
        );

        if let Some(jar_file) = &self.config.jar_file {
            println!("Jar file: {jar_file}");
        }

        println!(
            "Stop command configured: {}",
            yes_no(self.config.stop_command.is_some())
        );
        println!("RCON port: {}", optional_display(self.config.rcon_port));
        println!(
            "RCON password configured: {}",
            yes_no(self.config.rcon_password.is_some())
        );
        println!(
            "Backup enabled: {}",
            yes_no(self.config.backup.unwrap_or(false))
        );
        println!("Restore mode: {}", self.config.restore.unwrap_or_default());

        Ok(())
    }

    pub fn local_backup_status(&self) -> Result<LocalBackupStatus> {
        let path = self
            .local_backup_dir
            .as_ref()
            .map(|backup_root| backup_root.join(&self.config.name));
        let exists = path.as_ref().is_some_and(|path| path.exists());
        let latest_modified = if let Some(path) = path.as_ref().filter(|path| path.exists()) {
            latest_modified_in_tree(path)?
        } else {
            None
        };

        Ok(LocalBackupStatus {
            path,
            exists,
            latest_modified,
        })
    }

    pub fn backup_server(&self) -> Result<()> {
        let backup_root = self.backup_root()?;
        let destination = backup_root.join(&self.config.name);

        fs::create_dir_all(backup_root).with_context(|| {
            format!(
                "Failed to create backup directory '{}'",
                backup_root.display()
            )
        })?;

        let source = format_path_with_trailing_slash(&self.server_dir);
        info!(server = %self.config.name, source = %source, destination = %destination.display(), "starting server backup");

        run_rsync(&source, &destination, "backup", &self.config.name, false)?;

        info!(server = %self.config.name, destination = %destination.display(), "server backup completed");
        Ok(())
    }

    pub fn validate_remote_backup_environment(&self) -> Result<()> {
        self.remote_backup_provider().validate_environment()
    }

    pub fn remote_backup_server(&self) -> Result<()> {
        let metrics = self
            .remote_backup_provider()
            .backup_server(RemoteBackupContext {
                server_id: &self.server_id,
                server_name: &self.config.name,
                server_dir: &self.server_dir,
            })?;

        println!(
            "Remote backup metrics for '{}': {}",
            self.config.name,
            format_remote_backup_metrics(&metrics)
        );
        Ok(())
    }

    pub fn restore_server(&self, dry_run: bool) -> Result<()> {
        let mode = self.config.restore.unwrap_or_default();
        let (source, destination) = self.restore_paths(mode)?;

        ensure_path_exists(&source, "restore source")?;
        if dry_run {
            println!("Restore dry run for server '{}'", self.server_id);
            println!("Server: {}", self.config.name);
            println!("Mode: {mode}");
            println!("Source: {}", source.display());
            println!("Destination: {}", destination.display());
            println!("No files will be copied, overwritten, or deleted.");
        } else {
            confirm_restore(
                &self.server_id,
                &self.config.name,
                mode,
                &source,
                &destination,
            )?;
        }

        if !dry_run {
            if matches!(mode, RestoreMode::World) {
                fs::create_dir_all(&destination).with_context(|| {
                    format!(
                        "Failed to create restore destination directory '{}'",
                        destination.display()
                    )
                })?;
            } else if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "Failed to create restore destination parent directory '{}'",
                        parent.display()
                    )
                })?;
            }
        }

        let source = format_path_with_trailing_slash(&source);
        info!(server = %self.config.name, restore_mode = %mode, source = %source, destination = %destination.display(), dry_run, "starting server restore");

        run_rsync(&source, &destination, "restore", &self.config.name, dry_run)?;

        if dry_run {
            info!(server = %self.config.name, restore_mode = %mode, destination = %destination.display(), "server restore dry run completed");
        } else {
            info!(server = %self.config.name, restore_mode = %mode, destination = %destination.display(), "server restore completed");
        }
        Ok(())
    }

    fn backup_root(&self) -> Result<&Path> {
        self.local_backup_dir.as_deref().ok_or_else(|| {
            anyhow!(
                "Server '{}' requires backup.localDir for local backup/restore operations.",
                self.config.name
            )
        })
    }

    pub fn stage_remote_restore(
        &self,
        snapshot: &str,
        mode_override: Option<RestoreMode>,
        dry_run: bool,
    ) -> Result<RemoteRestorePlan> {
        self.validate_remote_backup_environment()?;
        let mode = mode_override.unwrap_or_else(|| self.config.restore.unwrap_or_default());
        let backup_root = self.backup_root()?;
        let temp_dir = backup_root.join(".remote-restore").join(format!(
            "{}-{}",
            self.server_id,
            Local::now().format("%Y-%m-%d_%H-%M-%S")
        ));
        let remote_include = self.remote_restore_include_path(mode);
        let staged_source = temp_dir.join(path_without_root(&remote_include));
        let destination = self.restore_destination(mode);

        println!(
            "Remote restore{} for server '{}'",
            if dry_run { " dry run" } else { "" },
            self.server_id
        );
        println!("Server: {}", self.config.name);
        println!("Snapshot: {snapshot}");
        println!("Mode: {mode}");
        println!("Remote include: {}", remote_include.display());
        println!("Temporary restore directory: {}", temp_dir.display());
        println!("Destination: {}", destination.display());
        if dry_run {
            println!(
                "Remote backup data will be staged temporarily, but no live server files will be changed and the server will not be stopped."
            );
        }

        fs::create_dir_all(&temp_dir).with_context(|| {
            format!(
                "Failed to create temporary restore directory '{}'",
                temp_dir.display()
            )
        })?;
        self.remote_backup_provider()
            .restore_snapshot(RemoteRestoreContext {
                server_id: &self.server_id,
                snapshot,
                include: &remote_include,
                target: &temp_dir,
            })
            .with_context(|| {
                format!(
                    "Remote restore staging failed for server '{}'",
                    self.config.name
                )
            })?;
        ensure_path_exists(&staged_source, "remote restore staged source")?;

        Ok(RemoteRestorePlan {
            temp_dir,
            staged_source,
            destination,
            mode,
        })
    }

    pub fn confirm_remote_restore(&self, plan: &RemoteRestorePlan) -> Result<()> {
        confirm_restore(
            &self.server_id,
            &self.config.name,
            plan.mode,
            &plan.staged_source,
            &plan.destination,
        )
    }

    pub fn apply_remote_restore(&self, plan: &RemoteRestorePlan, dry_run: bool) -> Result<()> {
        let restore_result = (|| -> Result<()> {
            if !dry_run {
                if matches!(plan.mode, RestoreMode::World) {
                    fs::create_dir_all(&plan.destination).with_context(|| {
                        format!(
                            "Failed to create restore destination directory '{}'",
                            plan.destination.display()
                        )
                    })?;
                } else if let Some(parent) = plan.destination.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!(
                            "Failed to create restore destination parent directory '{}'",
                            parent.display()
                        )
                    })?;
                }
            }

            let source = format_path_with_trailing_slash(&plan.staged_source);
            run_rsync(
                &source,
                &plan.destination,
                "remote restore",
                &self.config.name,
                dry_run,
            )
        })();

        match restore_result {
            Ok(()) => {
                self.cleanup_remote_restore_plan(plan);
                if dry_run {
                    info!(server = %self.config.name, restore_mode = %plan.mode, "remote restore dry run completed");
                } else {
                    info!(server = %self.config.name, restore_mode = %plan.mode, "remote restore completed");
                }
                Ok(())
            }
            Err(err) => Err(err).context(format!(
                "Remote restore failed; temporary restore data kept at '{}'",
                plan.temp_dir.display()
            )),
        }
    }

    pub fn cleanup_remote_restore_plan(&self, plan: &RemoteRestorePlan) {
        if let Err(err) = fs::remove_dir_all(&plan.temp_dir) {
            warn!(temp_dir = %plan.temp_dir.display(), error = %err, "failed to remove temporary remote restore directory");
        }
    }

    fn remote_backup_provider(&self) -> Box<dyn RemoteBackupProvider + '_> {
        remote_backup_provider(&self.backup_config)
    }

    fn remote_restore_include_path(&self, mode: RestoreMode) -> PathBuf {
        match mode {
            RestoreMode::World => self.server_dir.join("world"),
            RestoreMode::All => self.server_dir.clone(),
        }
    }

    fn restore_destination(&self, mode: RestoreMode) -> PathBuf {
        match mode {
            RestoreMode::World => self.server_dir.join("world"),
            RestoreMode::All => self.server_dir.clone(),
        }
    }

    fn restore_paths(&self, mode: RestoreMode) -> Result<(PathBuf, PathBuf)> {
        let backup_server_dir = self.backup_root()?.join(&self.config.name);

        Ok(match mode {
            RestoreMode::World => (
                backup_server_dir.join("world"),
                self.server_dir.join("world"),
            ),
            RestoreMode::All => (backup_server_dir, self.server_dir.clone()),
        })
    }
}

pub fn latest_remote_snapshot(
    backup: &BackupConfig,
    server_id: &str,
) -> Result<Option<crate::remote_backup::RemoteSnapshotSummary>> {
    remote_backup_provider(backup).latest_snapshot(server_id)
}

pub fn print_remote_snapshots(backup: &BackupConfig, server_id: &str, latest: usize) -> Result<()> {
    remote_backup_provider(backup).print_snapshots(server_id, latest)
}

pub fn format_system_time(time: SystemTime) -> String {
    let time: DateTime<Local> = time.into();
    time.format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn cleanup_remote_backups(backup: &BackupConfig, dry_run: bool) -> Result<()> {
    remote_backup_provider(backup).cleanup(dry_run)
}

pub fn restic_environment_status(backup: &BackupConfig) -> Result<()> {
    remote_backup_provider(backup).validate_environment()
}

fn run_rsync(
    source: &str,
    destination: &Path,
    operation: &str,
    server_name: &str,
    dry_run: bool,
) -> Result<()> {
    let mut command = Command::new("rsync");
    command.arg("-av").arg("--delete");
    if dry_run {
        command.arg("--dry-run");
    }
    let status = command
        .arg(source)
        .arg(destination)
        .status()
        .with_context(|| format!("Failed to run 'rsync' for server {operation}"))?;

    if !status.success() {
        error!(server = %server_name, operation, dry_run, exit_code = ?status.code(), "server rsync operation failed");
        bail!(
            "{} failed for server '{}'. Return code: {:?}",
            capitalize(operation),
            server_name,
            status.code()
        );
    }

    Ok(())
}

fn ensure_path_exists(path: &Path, label: &str) -> Result<()> {
    fs::metadata(path).with_context(|| {
        format!(
            "{} '{}' does not exist or is unreadable",
            label,
            path.display()
        )
    })?;
    Ok(())
}

fn confirm_restore(
    server_id: &str,
    server_name: &str,
    mode: RestoreMode,
    source: &Path,
    destination: &Path,
) -> Result<()> {
    println!("Restore confirmation required");
    println!("ID: {server_id}");
    println!("Server: {server_name}");
    println!("Mode: {mode}");
    println!("Source: {}", source.display());
    println!("Destination: {}", destination.display());
    println!(
        "This will overwrite destination files and delete destination files that are not present in the backup."
    );

    if !io::stdin().is_terminal() {
        bail!("Cannot confirm restore because stdin is not a terminal");
    }

    print!("Type 'restore {server_id}' to continue: ");
    io::stdout().flush().context("Failed to flush stdout")?;

    let mut response = String::new();
    io::stdin()
        .read_line(&mut response)
        .context("Failed to read restore confirmation")?;

    if response.trim() == format!("restore {server_id}") {
        Ok(())
    } else {
        bail!("Restore cancelled");
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn path_without_root(path: &Path) -> PathBuf {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect()
}

fn format_path_with_trailing_slash(path: &Path) -> String {
    let mut text = path.display().to_string();
    if !text.ends_with(std::path::MAIN_SEPARATOR) {
        text.push(std::path::MAIN_SEPARATOR);
    }
    text
}

fn path_status(path: &PathBuf) -> Result<&'static str> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok("exists"),
        Ok(_) => Ok("exists, but is not a directory"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok("missing"),
        Err(err) => Err(err).context("Failed to read path metadata"),
    }
}

fn latest_modified_in_tree(path: &Path) -> Result<Option<SystemTime>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("Failed to read metadata for '{}'", path.display()));
        }
    };

    let mut latest =
        Some(metadata.modified().with_context(|| {
            format!("Failed to read modification time for '{}'", path.display())
        })?);

    if metadata.is_dir() {
        for entry in fs::read_dir(path)
            .with_context(|| format!("Failed to read directory '{}'", path.display()))?
        {
            let entry = entry.context("Failed to read directory entry")?;
            if let Some(modified) = latest_modified_in_tree(&entry.path())?
                && latest.is_none_or(|latest_time| modified > latest_time)
            {
                latest = Some(modified);
            }
        }
    }

    Ok(latest)
}

fn latest_file_in_directory(directory: &PathBuf) -> Result<Option<PathBuf>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context("Failed to read directory"),
    };

    let mut latest_file = None;
    let mut latest_modified = SystemTime::UNIX_EPOCH;

    for entry in entries {
        let entry = entry.context("Failed to read directory entry")?;
        let metadata = entry
            .metadata()
            .with_context(|| format!("Failed to read metadata for '{}'", entry.path().display()))?;

        if !metadata.is_file() {
            continue;
        }

        let modified = metadata.modified().with_context(|| {
            format!(
                "Failed to read modification time for '{}'",
                entry.path().display()
            )
        })?;

        if modified >= latest_modified {
            latest_modified = modified;
            latest_file = Some(entry.path());
        }
    }

    Ok(latest_file)
}

fn optional_display<T: std::fmt::Display>(value: Option<T>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "not configured".to_string())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn screen_session_exists_in_output(output: &str, expected_session_name: &str) -> bool {
    output
        .lines()
        .filter_map(parse_screen_session_name)
        .any(|session_name| session_name == expected_session_name)
}

fn parse_screen_session_name(line: &str) -> Option<&str> {
    let first_field = line.split_whitespace().next()?;
    let (_, session_name) = first_field.split_once('.')?;

    if session_name.is_empty() {
        None
    } else {
        Some(session_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_exact_screen_session_name() {
        let output = r#"
            There are screens on:
                1234.survival	(06/10/2026 10:00:00 AM)	(Detached)
                5678.velocity	(06/10/2026 10:00:00 AM)	(Detached)
            2 Sockets in /run/screen/S-user.
        "#;

        assert!(screen_session_exists_in_output(output, "survival"));
        assert!(screen_session_exists_in_output(output, "velocity"));
    }

    #[test]
    fn does_not_match_partial_screen_session_name() {
        let output = r#"
            There is a screen on:
                1234.survival-backup	(06/10/2026 10:00:00 AM)	(Detached)
            1 Socket in /run/screen/S-user.
        "#;

        assert!(!screen_session_exists_in_output(output, "survival"));
    }

    #[test]
    fn ignores_non_session_lines() {
        let output = r#"
            No Sockets found in /run/screen/S-user.
        "#;

        assert!(!screen_session_exists_in_output(output, "survival"));
    }

    #[test]
    fn formats_backup_source_with_trailing_slash() {
        let path = PathBuf::from("/srv/servers/survival");

        assert_eq!(
            format_path_with_trailing_slash(&path),
            "/srv/servers/survival/"
        );
    }

    #[test]
    fn capitalizes_operation_name() {
        assert_eq!(capitalize("restore"), "Restore");
    }
}
