use serde::Deserialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub projects_dir: PathBuf,
    #[allow(dead_code)]
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    pub base_url: String,
    pub model: String,
    pub api_key_file: PathBuf,
    pub request_timeout_seconds: u64,
}

impl Config {
    pub fn load(path: &Path) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let mut config: Self = toml::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if config.projects_dir.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "projects_dir must not be empty",
            ));
        }
        if config.projects_dir.is_relative() {
            config.projects_dir = path
                .parent()
                .unwrap_or(Path::new("."))
                .join(config.projects_dir);
        }
        if config.llm.api_key_file.is_relative() {
            config.llm.api_key_file = path
                .parent()
                .unwrap_or(Path::new("."))
                .join(config.llm.api_key_file);
        }
        Ok(config)
    }
}
