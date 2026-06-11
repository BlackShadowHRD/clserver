pub mod generic;
pub mod manager;
pub mod minecraft;

use anyhow::{Context, Result, anyhow, bail};

use crate::cli::{Action, BackupTarget, Request, StopType};
use crate::config::{self, Config, ServerType};
use chrono::{Datelike, Local, Weekday};
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

use generic::GenericServer;
use manager::{ServerManager, cleanup_remote_backups};
use minecraft::MinecraftServer;

const MAINTENANCE_STOP_TIMEOUT: Duration = Duration::from_secs(900);
const MAINTENANCE_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub fn dispatch_request(request: Request, mut config: Config) -> Result<()> {
    if let Action::ValidateConfig { fix } = request.action {
        return config::validate_or_fix_minecraft_rcon_passwords(&config, fix);
    }

    if matches!(request.action, Action::List) {
        return list_servers(&config);
    }

    match &request.action {
        Action::Maintenance => return run_maintenance(&mut config),
        Action::BackupLocal {
            target: BackupTarget::All,
        } => return backup_all_servers(&mut config, BackupKind::Local),
        Action::BackupRemote {
            target: BackupTarget::All,
        } => return backup_all_servers(&mut config, BackupKind::Remote),
        Action::BackupCleanup => return cleanup_remote_backups(&config.backup),
        _ => {}
    }

    let server_name = request
        .server
        .as_deref()
        .ok_or_else(|| anyhow!("This action requires a server name."))?;
    let server_type = config
        .servers
        .get(server_name)
        .map(|server| server.server_type)
        .ok_or_else(|| anyhow!("Server '{}' not found in configuration file.", server_name))?;

    if server_type == ServerType::Minecraft && action_needs_minecraft_rcon(&request.action) {
        config::reconcile_minecraft_rcon_password(&mut config, server_name)?;
    }

    let server_config = config
        .servers
        .get(server_name)
        .cloned()
        .ok_or_else(|| anyhow!("Server '{}' not found in configuration file.", server_name))?;

    match server_config.server_type {
        ServerType::Minecraft => {
            let server = MinecraftServer::new(
                server_name.to_string(),
                server_config,
                &config.global,
                &config.backup,
                &config.java_environments,
            )?;
            dispatch_minecraft(&server, request.action)
        }
        ServerType::Velocity | ServerType::Hytale => {
            let server = GenericServer::new(
                server_name.to_string(),
                server_config,
                &config.global,
                &config.backup,
                &config.java_environments,
            )?;
            dispatch_generic(&server, request.action)
        }
    }
}

fn action_needs_minecraft_rcon(action: &Action) -> bool {
    matches!(
        action,
        Action::Stop { .. }
            | Action::Restart
            | Action::BackupLocal { .. }
            | Action::BackupRemote { .. }
            | Action::Restore
    )
}

fn dispatch_minecraft(server: &MinecraftServer, action: Action) -> Result<()> {
    match action {
        Action::Start => server.start_server(),
        Action::Stop { stop_type } => server.stop_server(stop_type),
        Action::BackupLocal { .. } => backup_minecraft_server(server, BackupKind::Local),
        Action::BackupRemote { .. } => backup_minecraft_server(server, BackupKind::Remote),
        Action::Restore => restore_minecraft_server(server),
        Action::Restart => server.restart_server(),
        Action::Attach => server.manager.attach_server(),
        Action::Status => server.manager.status_server(),
        Action::BackupCleanup
        | Action::Maintenance
        | Action::List
        | Action::ValidateConfig { .. } => no_server_action_unreachable(),
    }
}

fn dispatch_generic(server: &GenericServer, action: Action) -> Result<()> {
    match action {
        Action::Start => server.manager.start_server(),
        Action::Stop { stop_type } => {
            if server.manager.config.server_type == ServerType::Velocity
                || stop_type == StopType::Immediate
            {
                server.manager.stop_with_stop_command()
            } else {
                warn!(
                    stop_type = ?stop_type,
                    server_type = %server.manager.config.server_type,
                    "unsupported stop type for server type; falling back to configured stop command"
                );
                server.manager.stop_with_stop_command()
            }
        }
        Action::BackupLocal { .. } => backup_generic_server(&server.manager, BackupKind::Local),
        Action::BackupRemote { .. } => backup_generic_server(&server.manager, BackupKind::Remote),
        Action::Restore => restore_generic_server(&server.manager),
        Action::Restart => server.manager.restart_with_stop_command(),
        Action::Attach => server.manager.attach_server(),
        Action::Status => server.manager.status_server(),
        Action::BackupCleanup
        | Action::Maintenance
        | Action::List
        | Action::ValidateConfig { .. } => no_server_action_unreachable(),
    }
}

#[derive(Clone, Copy)]
enum BackupKind {
    Local,
    Remote,
}

fn backup_minecraft_server(server: &MinecraftServer, kind: BackupKind) -> Result<()> {
    if matches!(kind, BackupKind::Remote) {
        server.manager.validate_remote_backup_environment()?;
    }

    let was_running = server.manager.screen_session_exists()?;

    if was_running {
        server.stop_server(StopType::Friendly)?;
        ensure_manager_stopped(&server.manager)?;
    }

    let backup_result = run_manager_backup(&server.manager, kind);

    if was_running && let Err(start_err) = server.start_server() {
        return match backup_result {
            Ok(()) => Err(start_err).context("Backup completed, but failed to restart server"),
            Err(backup_err) => Err(backup_err).context(format!(
                "Backup failed, and server restart also failed: {start_err:#}"
            )),
        };
    }

    backup_result
}

fn backup_generic_server(manager: &ServerManager, kind: BackupKind) -> Result<()> {
    if matches!(kind, BackupKind::Remote) {
        manager.validate_remote_backup_environment()?;
    }

    let was_running = manager.screen_session_exists()?;

    if was_running {
        manager.stop_with_stop_command()?;
        ensure_manager_stopped(manager)?;
    }

    let backup_result = run_manager_backup(manager, kind);

    if was_running && let Err(start_err) = manager.start_server() {
        return match backup_result {
            Ok(()) => Err(start_err).context("Backup completed, but failed to restart server"),
            Err(backup_err) => Err(backup_err).context(format!(
                "Backup failed, and server restart also failed: {start_err:#}"
            )),
        };
    }

    backup_result
}

fn run_manager_backup(manager: &ServerManager, kind: BackupKind) -> Result<()> {
    match kind {
        BackupKind::Local => manager.backup_server(),
        BackupKind::Remote => manager.remote_backup_server(),
    }
}

fn restore_minecraft_server(server: &MinecraftServer) -> Result<()> {
    let was_running = server.manager.screen_session_exists()?;

    if was_running {
        server.stop_server(StopType::Friendly)?;
        ensure_manager_stopped(&server.manager)?;
    }

    let restore_result = server.manager.restore_server();

    if was_running && let Err(start_err) = server.start_server() {
        return match restore_result {
            Ok(()) => Err(start_err).context("Restore completed, but failed to restart server"),
            Err(restore_err) => Err(restore_err).context(format!(
                "Restore failed, and server restart also failed: {start_err:#}"
            )),
        };
    }

    restore_result
}

fn restore_generic_server(manager: &ServerManager) -> Result<()> {
    let was_running = manager.screen_session_exists()?;

    if was_running {
        manager.stop_with_stop_command()?;
        ensure_manager_stopped(manager)?;
    }

    let restore_result = manager.restore_server();

    if was_running && let Err(start_err) = manager.start_server() {
        return match restore_result {
            Ok(()) => Err(start_err).context("Restore completed, but failed to restart server"),
            Err(restore_err) => Err(restore_err).context(format!(
                "Restore failed, and server restart also failed: {start_err:#}"
            )),
        };
    }

    restore_result
}

fn run_maintenance(config: &mut Config) -> Result<()> {
    info!("daily maintenance started");
    println!("Daily maintenance started");

    reconcile_running_minecraft_servers(config)?;
    handle_velocity_servers(config)?;

    let should_run_cleanup = config
        .servers
        .values()
        .any(|server| server.backup.unwrap_or(false));
    let tasks = build_maintenance_tasks(config)?;
    let task_count = tasks.len();
    info!(task_count, "starting parallel server maintenance tasks");

    let handles: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            thread::spawn(move || {
                let server_id = task.server.server_id().to_string();
                let result = process_maintenance_task(task);
                (server_id, result)
            })
        })
        .collect();

    let mut failures = Vec::new();
    for handle in handles {
        let (server_id, result) = handle
            .join()
            .map_err(|_| anyhow!("maintenance worker thread panicked"))?;

        if let Err(err) = result {
            failures.push(format!("{server_id}: {err:#}"));
        }
    }

    if failures.is_empty() {
        if is_monday() && should_run_cleanup {
            cleanup_remote_backups(&config.backup)
                .context("Daily maintenance backups completed, but remote cleanup failed")?;
        }

        info!(task_count, "daily maintenance finished successfully");
        println!("Daily maintenance finished");
        Ok(())
    } else {
        for failure in &failures {
            tracing::error!(%failure, "maintenance task failed");
        }
        bail!("Daily maintenance failed:\n- {}", failures.join("\n- "));
    }
}

fn backup_all_servers(config: &mut Config, kind: BackupKind) -> Result<()> {
    reconcile_running_minecraft_servers(config)?;

    let mut server_ids: Vec<_> = config
        .servers
        .iter()
        .filter(|(_, server)| server.backup.unwrap_or(false))
        .map(|(server_id, _)| server_id.clone())
        .collect();
    server_ids.sort();

    let tasks = server_ids
        .into_iter()
        .map(|server_id| {
            let server = maintenance_server_for_config(config, &server_id)?;
            Ok(BackupTask { server, kind })
        })
        .collect::<Result<Vec<_>>>()?;

    run_backup_tasks(tasks)
}

fn run_backup_tasks(tasks: Vec<BackupTask>) -> Result<()> {
    let handles: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            thread::spawn(move || {
                let server_id = task.server.server_id().to_string();
                let result = process_backup_task(task);
                (server_id, result)
            })
        })
        .collect();

    let mut failures = Vec::new();
    for handle in handles {
        let (server_id, result) = handle
            .join()
            .map_err(|_| anyhow!("backup worker thread panicked"))?;

        if let Err(err) = result {
            failures.push(format!("{server_id}: {err:#}"));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        bail!("Backup failed:\n- {}", failures.join("\n- "));
    }
}

struct BackupTask {
    server: MaintenanceServer,
    kind: BackupKind,
}

fn process_backup_task(task: BackupTask) -> Result<()> {
    if matches!(task.kind, BackupKind::Remote) {
        task.server.validate_remote_backup_environment()?;
    }

    let was_running = task.server.screen_session_exists()?;

    if was_running {
        task.server.stop_for_maintenance()?;
        ensure_stopped(&task.server)?;
    }

    let backup_result = task.server.backup_server(task.kind);

    if was_running && let Err(start_err) = task.server.start_server() {
        return match backup_result {
            Ok(()) => Err(start_err).context("Backup completed, but failed to restart server"),
            Err(backup_err) => Err(backup_err).context(format!(
                "Backup failed, and server restart also failed: {start_err:#}"
            )),
        };
    }

    backup_result
}

fn is_monday() -> bool {
    Local::now().weekday() == Weekday::Mon
}

fn reconcile_running_minecraft_servers(config: &mut Config) -> Result<()> {
    let running_minecraft_ids =
        running_server_ids(config, |server_type| server_type == ServerType::Minecraft)?;

    for server_id in running_minecraft_ids {
        config::reconcile_minecraft_rcon_password(config, &server_id)?;
    }

    Ok(())
}

fn handle_velocity_servers(config: &Config) -> Result<()> {
    let mut velocity_ids: Vec<_> = config
        .servers
        .iter()
        .filter(|(_, server)| server.server_type == ServerType::Velocity)
        .map(|(server_id, _)| server_id.clone())
        .collect();
    velocity_ids.sort();

    for server_id in velocity_ids {
        let server_config = config
            .servers
            .get(&server_id)
            .ok_or_else(|| anyhow!("Server '{}' not found in configuration file.", server_id))?;
        let manager = manager_for_server(config, &server_id)?;
        let was_running = manager.screen_session_exists()?;
        let should_start = server_config.enabled.unwrap_or(false);

        if was_running {
            info!(server = %manager.config.name, id = %manager.server_id, "stopping velocity server before backend maintenance");
            println!("Stopping Velocity server {}", manager.server_id);
            manager.stop_with_stop_command()?;
            ensure_manager_stopped(&manager)?;
        }

        if server_config.backup.unwrap_or(false) {
            manager.validate_remote_backup_environment()?;
        }

        let backup_result = if server_config.backup.unwrap_or(false) {
            manager
                .backup_server()
                .and_then(|()| manager.remote_backup_server())
        } else {
            Ok(())
        };

        if was_running || should_start {
            info!(server = %manager.config.name, id = %manager.server_id, "starting velocity server before backend maintenance");
            println!("Starting Velocity server {}", manager.server_id);
            if let Err(start_err) = manager.start_server() {
                return match backup_result {
                    Ok(()) => Err(start_err)
                        .context("Velocity backup completed, but failed to restart server"),
                    Err(backup_err) => Err(backup_err).context(format!(
                        "Velocity backup failed, and server restart also failed: {start_err:#}"
                    )),
                };
            }
        }

        backup_result?;
    }

    Ok(())
}

fn build_maintenance_tasks(config: &Config) -> Result<Vec<MaintenanceTask>> {
    let mut server_ids: Vec<_> = config.servers.keys().cloned().collect();
    server_ids.sort();

    let mut tasks = Vec::new();
    for server_id in server_ids {
        let server_config = config
            .servers
            .get(&server_id)
            .ok_or_else(|| anyhow!("Server '{}' not found in configuration file.", server_id))?;

        if server_config.server_type == ServerType::Velocity {
            continue;
        }

        let server = maintenance_server_for_config(config, &server_id)?;
        let was_running = server.screen_session_exists()?;
        let should_start = server_config.enabled.unwrap_or(false);
        let should_backup = server_config.backup.unwrap_or(false);

        if was_running || should_start || should_backup {
            tasks.push(MaintenanceTask {
                server,
                was_running,
                should_start,
                should_backup,
            });
        }
    }

    Ok(tasks)
}

fn running_server_ids(
    config: &Config,
    server_type_matches: impl Fn(ServerType) -> bool,
) -> Result<Vec<String>> {
    let mut ids = Vec::new();

    for (server_id, server_config) in &config.servers {
        if !server_type_matches(server_config.server_type) {
            continue;
        }

        let manager = manager_for_server(config, server_id)?;
        if manager.screen_session_exists()? {
            ids.push(server_id.clone());
        }
    }

    Ok(ids)
}

struct MaintenanceTask {
    server: MaintenanceServer,
    was_running: bool,
    should_start: bool,
    should_backup: bool,
}

enum MaintenanceServer {
    Minecraft(MinecraftServer),
    Generic(ServerManager),
}

impl MaintenanceServer {
    fn server_id(&self) -> &str {
        match self {
            Self::Minecraft(server) => &server.manager.server_id,
            Self::Generic(manager) => &manager.server_id,
        }
    }

    fn server_name(&self) -> &str {
        match self {
            Self::Minecraft(server) => &server.manager.config.name,
            Self::Generic(manager) => &manager.config.name,
        }
    }

    fn screen_session_exists(&self) -> Result<bool> {
        match self {
            Self::Minecraft(server) => server.manager.screen_session_exists(),
            Self::Generic(manager) => manager.screen_session_exists(),
        }
    }

    fn stop_for_maintenance(&self) -> Result<()> {
        match self {
            Self::Minecraft(server) => server.stop_server(StopType::Friendly),
            Self::Generic(manager) => manager.stop_with_stop_command(),
        }
    }

    fn wait_until_stopped(&self) -> Result<bool> {
        match self {
            Self::Minecraft(server) => server
                .manager
                .wait_until_stopped(MAINTENANCE_STOP_TIMEOUT, MAINTENANCE_POLL_INTERVAL),
            Self::Generic(manager) => {
                manager.wait_until_stopped(MAINTENANCE_STOP_TIMEOUT, MAINTENANCE_POLL_INTERVAL)
            }
        }
    }

    fn validate_remote_backup_environment(&self) -> Result<()> {
        match self {
            Self::Minecraft(server) => server.manager.validate_remote_backup_environment(),
            Self::Generic(manager) => manager.validate_remote_backup_environment(),
        }
    }

    fn backup_server(&self, kind: BackupKind) -> Result<()> {
        match (self, kind) {
            (Self::Minecraft(server), BackupKind::Local) => server.manager.backup_server(),
            (Self::Minecraft(server), BackupKind::Remote) => server.manager.remote_backup_server(),
            (Self::Generic(manager), BackupKind::Local) => manager.backup_server(),
            (Self::Generic(manager), BackupKind::Remote) => manager.remote_backup_server(),
        }
    }

    fn start_server(&self) -> Result<()> {
        match self {
            Self::Minecraft(server) => server.start_server(),
            Self::Generic(manager) => manager.start_server(),
        }
    }
}

fn maintenance_server_for_config(config: &Config, server_id: &str) -> Result<MaintenanceServer> {
    let server_config = config
        .servers
        .get(server_id)
        .cloned()
        .ok_or_else(|| anyhow!("Server '{}' not found in configuration file.", server_id))?;

    match server_config.server_type {
        ServerType::Minecraft => Ok(MaintenanceServer::Minecraft(MinecraftServer::new(
            server_id.to_string(),
            server_config,
            &config.global,
            &config.backup,
            &config.java_environments,
        )?)),
        ServerType::Velocity | ServerType::Hytale => {
            Ok(MaintenanceServer::Generic(ServerManager::new(
                server_id.to_string(),
                server_config,
                &config.global,
                &config.backup,
                &config.java_environments,
            )?))
        }
    }
}

fn process_maintenance_task(task: MaintenanceTask) -> Result<()> {
    if task.should_backup {
        task.server.validate_remote_backup_environment()?;
    }

    let server_id = task.server.server_id().to_string();
    let server_name = task.server.server_name().to_string();
    info!(server = %server_name, id = %server_id, "maintenance task started");
    println!("Maintaining {server_id}");

    if task.was_running {
        task.server.stop_for_maintenance()?;
        ensure_stopped(&task.server)?;
    }

    if task.should_backup {
        task.server.backup_server(BackupKind::Local)?;
        task.server.backup_server(BackupKind::Remote)?;
    }

    if task.should_start {
        task.server.start_server()?;
    }

    info!(server = %server_name, id = %server_id, "maintenance task finished");
    Ok(())
}

fn ensure_stopped(server: &MaintenanceServer) -> Result<()> {
    if server.wait_until_stopped()? {
        Ok(())
    } else {
        bail!(
            "Timed out waiting for server '{}' to stop",
            server.server_name()
        );
    }
}

fn ensure_manager_stopped(manager: &ServerManager) -> Result<()> {
    if manager.wait_until_stopped(MAINTENANCE_STOP_TIMEOUT, MAINTENANCE_POLL_INTERVAL)? {
        Ok(())
    } else {
        bail!(
            "Timed out waiting for server '{}' to stop",
            manager.config.name
        );
    }
}

fn manager_for_server(config: &Config, server_id: &str) -> Result<ServerManager> {
    let server_config = config
        .servers
        .get(server_id)
        .cloned()
        .ok_or_else(|| anyhow!("Server '{}' not found in configuration file.", server_id))?;

    ServerManager::new(
        server_id.to_string(),
        server_config,
        &config.global,
        &config.backup,
        &config.java_environments,
    )
    .with_context(|| format!("Failed to initialize server manager for '{server_id}'"))
}

fn list_servers(config: &Config) -> Result<()> {
    let mut servers: Vec<_> = config.servers.iter().collect();
    servers.sort_by(|(a_id, a), (b_id, b)| a_id.cmp(b_id).then_with(|| a.name.cmp(&b.name)));

    let rows = servers
        .into_iter()
        .map(|(server_id, server_config)| {
            let manager = ServerManager::new(
                server_id.clone(),
                server_config.clone(),
                &config.global,
                &config.backup,
                &config.java_environments,
            )?;
            let status = if manager.screen_session_exists()? {
                "running"
            } else {
                "stopped"
            };

            Ok(ListRow {
                id: manager.server_id,
                server: manager.config.name,
                server_type: manager.config.server_type.to_string(),
                status: status.to_string(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    print_list_rows(&rows);
    Ok(())
}

struct ListRow {
    id: String,
    server: String,
    server_type: String,
    status: String,
}

fn print_list_rows(rows: &[ListRow]) {
    let id_width = list_column_width("ID", rows.iter().map(|row| row.id.as_str()));
    let server_width = list_column_width("SERVER", rows.iter().map(|row| row.server.as_str()));
    let type_width = list_column_width("TYPE", rows.iter().map(|row| row.server_type.as_str()));

    println!(
        "{}",
        format_list_row(
            "ID",
            "SERVER",
            "TYPE",
            "STATUS",
            id_width,
            server_width,
            type_width
        )
    );

    for row in rows {
        println!(
            "{}",
            format_list_row(
                &row.id,
                &row.server,
                &row.server_type,
                &row.status,
                id_width,
                server_width,
                type_width,
            )
        );
    }
}

fn list_column_width<'a>(header: &str, values: impl Iterator<Item = &'a str>) -> usize {
    values.map(str::len).max().unwrap_or(0).max(header.len())
}

fn format_list_row(
    id: &str,
    server: &str,
    server_type: &str,
    status: &str,
    id_width: usize,
    server_width: usize,
    type_width: usize,
) -> String {
    format!("{id:<id_width$}  {server:<server_width$}  {server_type:<type_width$}  {status}",)
}

fn no_server_action_unreachable() -> Result<()> {
    unreachable!("no-server actions are handled before per-server dispatch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_column_width_uses_longest_value_or_header() {
        assert_eq!(list_column_width("ID", ["CLS4", "proxy"].into_iter()), 5);
        assert_eq!(list_column_width("SERVER", ["one", "two"].into_iter()), 6);
    }

    #[test]
    fn formats_list_rows_with_dynamic_widths() {
        let row = format_list_row(
            "very-long-id",
            "CatLordSurvival",
            "minecraft",
            "running",
            "very-long-id".len(),
            "CatLordSurvival".len(),
            "minecraft".len(),
        );

        assert_eq!(row, "very-long-id  CatLordSurvival  minecraft  running");
    }
}
