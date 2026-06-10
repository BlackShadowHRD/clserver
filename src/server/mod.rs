pub mod generic;
pub mod manager;
pub mod minecraft;

use anyhow::{Result, anyhow};

use crate::cli::{Action, Request, StopType};
use crate::config::{Config, ServerType};
use tracing::warn;

use generic::GenericServer;
use minecraft::MinecraftServer;

pub fn dispatch_request(request: Request, config: Config) -> Result<()> {
    let server_config = config
        .servers
        .get(&request.server)
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "Server '{}' not found in configuration file.",
                request.server
            )
        })?;

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

fn dispatch_minecraft(server: &MinecraftServer, action: Action) -> Result<()> {
    match action {
        Action::Start => server.start_server(),
        Action::Stop { stop_type } => server.stop_server(stop_type),
        Action::Backup => server.manager.backup_server(),
        Action::Restart => server.restart_server(),
        Action::Attach => server.manager.attach_server(),
        Action::Status => server.manager.status_server(),
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
    }
}
