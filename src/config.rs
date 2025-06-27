use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;

const APP_IDENTIFIER: &str = "turso";

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Config {
    pub username: String,
    pub cache: Cache,
}
#[derive(Debug, Deserialize)]
pub struct Cache {
    pub database_names: Option<DatabaseNames>,
    pub database_token: Option<HashMap<String, DatabaseToken>>,
}
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DatabaseToken {
    pub expiration: u64,
    pub data: String,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseNames {
    pub data: Vec<DatabaseName>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseName {
    #[serde(alias = "dbid", alias = "dbId")]
    pub db_id: String,
    #[serde(alias = "Name")]
    pub name: String,
    #[serde(alias = "Hostname")]
    pub hostname: String,
}

pub fn load_config() -> anyhow::Result<Config> {
    let path = dirs::config_dir().ok_or(anyhow::anyhow!("No config dir"))?;
    let path = path.join(APP_IDENTIFIER);
    let path = path.join("settings.json");

    let config = std::fs::read_to_string(path).context("No Turso config found")?;
    Ok(serde_json::from_str(&config)?)
}

pub fn select_database(config: &Config) -> anyhow::Result<&DatabaseName> {
    let database_names = config.cache.database_names.as_ref().ok_or(anyhow::anyhow!(
        "No database names, please run `turso db list`"
    ))?;
    let databases: Vec<&str> = database_names
        .data
        .iter()
        .map(|d| d.name.as_str())
        .collect::<Vec<_>>();

    let selected_database =
        dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Select database")
            .items(&databases)
            .default(0)
            .interact()?;

    Ok(&database_names.data[selected_database])
}
