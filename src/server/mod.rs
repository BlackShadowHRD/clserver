pub mod generic;
pub mod manager;
pub mod minecraft;

use anyhow::{Result, anyhow};

use crate::cli::{Action, Request, StopType};
use crate::config::{self, Config, ServerType};
use tracing::warn;

use generic::GenericServer;
use manager::ServerManager;
use minecraft::MinecraftServer;

pub fn dispatch_request(request: Request, mut config: Config) -> Result<()> {
    if let Action::ValidateConfig { fix } = request.action {
        return config::validate_or_fix_minecraft_rcon_passwords(&config, fix);
    }

    if matches!(request.action, Action::List) {
        return list_servers(&config);
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
            let server =
                MinecraftServer::new(server_config, &config.global, &config.java_environments)?;
            dispatch_minecraft(&server, request.action)
        }
        ServerType::Velocity | ServerType::Hytale => {
            let server =
                GenericServer::new(server_config, &config.global, &config.java_environments)?;
            dispatch_generic(&server, request.action)
        }
    }
}

fn action_needs_minecraft_rcon(action: &Action) -> bool {
    matches!(action, Action::Stop { .. } | Action::Restart)
}

fn dispatch_minecraft(server: &MinecraftServer, action: Action) -> Result<()> {
    match action {
        Action::Start => server.start_server(),
        Action::Stop { stop_type } => server.stop_server(stop_type),
        Action::Backup => server.manager.backup_server(),
        Action::Restart => server.restart_server(),
        Action::Attach => server.manager.attach_server(),
        Action::Status => server.manager.status_server(),
        Action::List | Action::ValidateConfig { .. } => no_server_action_unreachable(),
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
        Action::Backup => server.manager.backup_server(),
        Action::Restart => server.manager.restart_with_stop_command(),
        Action::Attach => server.manager.attach_server(),
        Action::Status => server.manager.status_server(),
        Action::List | Action::ValidateConfig { .. } => no_server_action_unreachable(),
    }
}

fn list_servers(config: &Config) -> Result<()> {
    let mut servers: Vec<_> = config.servers.values().cloned().collect();
    servers.sort_by(|a, b| a.name.cmp(&b.name));

    println!("{:<24} {:<10} STATUS", "SERVER", "TYPE");
    for server_config in servers {
        let manager = ServerManager::new(server_config, &config.global, &config.java_environments)?;
        let status = if manager.screen_session_exists()? {
            "running"
        } else {
            "stopped"
        };

        println!(
            "{:<24} {:<10} {}",
            manager.config.name, manager.config.server_type, status
        );
    }

    Ok(())
}

fn no_server_action_unreachable() -> Result<()> {
    unreachable!("no-server actions are handled before per-server dispatch")
}
