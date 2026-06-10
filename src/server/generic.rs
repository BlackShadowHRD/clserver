use anyhow::Result;
use std::collections::HashMap;

use crate::config::{GlobalConfig, ServerConfig};

use super::manager::ServerManager;

pub struct GenericServer {
    pub manager: ServerManager,
}

impl GenericServer {
    pub fn new(
        server_id: String,
        config: ServerConfig,
        global: &GlobalConfig,
        java_environments: &HashMap<String, String>,
    ) -> Result<Self> {
        let manager = ServerManager::new(server_id, config, global, java_environments)?;
        Ok(Self { manager })
    }
}
