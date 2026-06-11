use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::thread::sleep;
use std::time::Duration;

use crate::cli::StopType;
use crate::config::{BackupConfig, GlobalConfig, ServerConfig};
use crate::rcon::RconClient;
use tracing::{info, warn};

use super::manager::{DEFAULT_STOP_POLL_INTERVAL, DEFAULT_STOP_TIMEOUT, ServerManager};

pub struct MinecraftServer {
    pub manager: ServerManager,
    rcon_port: u16,
    rcon_password: String,
}

impl MinecraftServer {
    pub fn new(
        server_id: String,
        config: ServerConfig,
        global: &GlobalConfig,
        backup: &BackupConfig,
        java_environments: &HashMap<String, String>,
    ) -> Result<Self> {
        let rcon_port = config
            .rcon_port
            .ok_or_else(|| anyhow!("Minecraft server '{}' has no rconPort.", config.name))?;
        let rcon_password = config
            .rcon_password
            .clone()
            .ok_or_else(|| anyhow!("Minecraft server '{}' has no rconPassword.", config.name))?;
        let manager = ServerManager::new(server_id, config, global, backup, java_environments)?;

        Ok(Self {
            manager,
            rcon_port,
            rcon_password,
        })
    }

    pub fn start_server(&self) -> Result<()> {
        self.manager.start_server()
    }

    pub fn stop_server(&self, stop_type: StopType) -> Result<()> {
        match stop_type {
            StopType::Friendly => self.friendly_stop(),
            StopType::Immediate => self.immediate_stop(),
        }
    }

    pub fn stop_server_and_wait(&self, stop_type: StopType) -> Result<()> {
        self.stop_server(stop_type)?;
        self.manager
            .wait_until_stopped_or_timeout(DEFAULT_STOP_TIMEOUT, DEFAULT_STOP_POLL_INTERVAL)
    }

    fn friendly_stop(&self) -> Result<()> {
        if !self.manager.screen_session_exists()? {
            warn!(server = %self.manager.config.name, "screen session is not running; skipping minecraft stop");
            return Ok(());
        }

        let response = self.rcon_command("list").with_context(|| {
            format!(
                "Minecraft server '{}' has a running screen session, but RCON is not reachable for friendly stop",
                self.manager.config.name
            )
        })?;
        let player_count = parse_player_count(&response).unwrap_or(0);

        info!(
            server = %self.manager.config.name,
            player_count,
            "minecraft friendly shutdown started"
        );

        if player_count > 0 {
            self.rcon_command("msg @a Server is shutting down in 5 minutes")?;
            sleep(Duration::from_secs(240));
            self.rcon_command("msg @a Server is shutting down in 1 minute")?;
            sleep(Duration::from_secs(60));
        } else {
            info!(
                server = %self.manager.config.name,
                player_count,
                requested_stop = "friendly",
                effective_stop = "immediate",
                "no players online during friendly shutdown; skipping warning delay and stopping immediately"
            );
        }

        self.immediate_stop()
    }

    fn immediate_stop(&self) -> Result<()> {
        if !self.manager.screen_session_exists()? {
            warn!(server = %self.manager.config.name, "screen session is not running; skipping minecraft stop");
            return Ok(());
        }

        self.rcon_command("stop").with_context(|| {
            format!(
                "Minecraft server '{}' has a running screen session, but RCON stop failed",
                self.manager.config.name
            )
        })?;
        info!(server = %self.manager.config.name, "minecraft server stopping immediately");
        Ok(())
    }

    pub fn restart_server(&self) -> Result<()> {
        self.stop_server_and_wait(StopType::Immediate)?;
        self.start_server()
    }

    fn rcon_command(&self, command: &str) -> Result<String> {
        let mut client = RconClient::connect("127.0.0.1", self.rcon_port)?;
        client.login(&self.rcon_password)?;
        client.command(command)
    }
}

fn parse_player_count(response: &str) -> Option<u32> {
    response
        .split_whitespace()
        .find_map(|part| part.parse::<u32>().ok())
}
