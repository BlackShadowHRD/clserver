use std::path::Path;
use std::time::Duration;

use anyhow::Result;

use crate::config::{BackupConfig, RemoteBackupProviderConfig};

mod kopia;
mod process;
mod restic;

use kopia::KopiaRemoteBackupProvider;
use restic::ResticRemoteBackupProvider;

#[derive(Debug, Clone)]
pub struct RemoteSnapshotSummary {
    pub short_id: Option<String>,
    pub time: Option<String>,
}

pub struct RemoteBackupContext<'a> {
    pub server_id: &'a str,
    pub server_name: &'a str,
    pub server_dir: &'a Path,
}

pub struct RemoteRestoreContext<'a> {
    pub server_id: &'a str,
    pub snapshot: &'a str,
    pub include: &'a Path,
    pub target: &'a Path,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteBackupMetrics {
    pub duration: Duration,
    pub user_cpu: Option<Duration>,
    pub system_cpu: Option<Duration>,
    pub peak_memory_kib: Option<u64>,
    pub bytes_added: Option<u64>,
}

impl RemoteBackupMetrics {
    pub fn cpu_utilization_percent(&self) -> Option<f64> {
        let total_cpu = self.user_cpu? + self.system_cpu?;
        let wall_seconds = self.duration.as_secs_f64();
        (wall_seconds > 0.0).then(|| total_cpu.as_secs_f64() / wall_seconds * 100.0)
    }
}

pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else if duration.as_millis() < 1000 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{seconds}s")
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = UNITS[0];

    for next_unit in UNITS.iter().skip(1) {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next_unit;
    }

    if unit == "B" {
        format!("{bytes} {unit}")
    } else {
        format!("{value:.1} {unit}")
    }
}

pub fn format_memory_kib(kib: u64) -> String {
    format_bytes(kib.saturating_mul(1024))
}

pub fn format_remote_backup_metrics(metrics: &RemoteBackupMetrics) -> String {
    let cpu = metrics
        .cpu_utilization_percent()
        .map(|value| format!("{value:.1}%"))
        .unwrap_or_else(|| "unknown".to_string());
    let user_cpu = metrics
        .user_cpu
        .map(format_duration)
        .unwrap_or_else(|| "unknown".to_string());
    let system_cpu = metrics
        .system_cpu
        .map(format_duration)
        .unwrap_or_else(|| "unknown".to_string());
    let peak_memory = metrics
        .peak_memory_kib
        .map(format_memory_kib)
        .unwrap_or_else(|| "unknown".to_string());
    let added = metrics
        .bytes_added
        .map(format_bytes)
        .unwrap_or_else(|| "unknown".to_string());

    format!(
        "duration: {}; CPU: {cpu} (user {user_cpu}, system {system_cpu}); peak memory: {peak_memory}; added to repository: {added}",
        format_duration(metrics.duration)
    )
}

pub trait RemoteBackupProvider {
    fn name(&self) -> &'static str;
    fn validate_environment(&self) -> Result<()>;
    fn backup_server(&self, context: RemoteBackupContext<'_>) -> Result<RemoteBackupMetrics>;
    fn restore_snapshot(&self, context: RemoteRestoreContext<'_>) -> Result<()>;
    fn latest_snapshot(&self, server_id: &str) -> Result<Option<RemoteSnapshotSummary>>;
    fn print_snapshots(&self, server_id: &str, latest: usize) -> Result<()>;
    fn cleanup(&self, dry_run: bool) -> Result<()>;
}

pub fn remote_backup_provider(backup: &BackupConfig) -> Box<dyn RemoteBackupProvider + '_> {
    match backup.provider.unwrap_or_default() {
        RemoteBackupProviderConfig::Restic => Box::new(ResticRemoteBackupProvider::new(
            backup.restic_env_file.as_deref(),
        )),
        RemoteBackupProviderConfig::Kopia => Box::new(KopiaRemoteBackupProvider::new(
            backup.kopia_env_file.as_deref(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_cpu_utilization_percent() {
        let metrics = RemoteBackupMetrics {
            duration: Duration::from_secs(10),
            user_cpu: Some(Duration::from_secs(7)),
            system_cpu: Some(Duration::from_secs(3)),
            peak_memory_kib: None,
            bytes_added: None,
        };

        assert_eq!(metrics.cpu_utilization_percent(), Some(100.0));
    }

    #[test]
    fn allows_multi_core_cpu_utilization_above_one_hundred_percent() {
        let metrics = RemoteBackupMetrics {
            duration: Duration::from_secs(10),
            user_cpu: Some(Duration::from_secs(20)),
            system_cpu: Some(Duration::from_secs(5)),
            peak_memory_kib: None,
            bytes_added: None,
        };

        assert_eq!(metrics.cpu_utilization_percent(), Some(250.0));
    }
}
