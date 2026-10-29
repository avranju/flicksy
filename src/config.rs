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
    pub api_key_file: Option<PathBuf>,
    pub api_key_env: Option<String>,
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
        if let Some(key_file) = &mut config.llm.api_key_file {
            if key_file.is_relative() {
                *key_file = path.parent().unwrap_or(Path::new(".")).join(&*key_file);
            }
        }
        Ok(config)
    }
}

impl LlmConfig {
    pub async fn api_key(&self) -> io::Result<Option<String>> {
        let value = if let Some(variable) = &self.api_key_env {
            Some(std::env::var(variable).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Cannot read API key environment variable {variable}: {error}"),
                )
            })?)
        } else if let Some(path) = &self.api_key_file {
            Some(tokio::fs::read_to_string(path).await?)
        } else {
            None
        };
        match value {
            Some(value) if value.trim().is_empty() => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Configured API key is empty",
            )),
            Some(value) => Ok(Some(value.trim().to_owned())),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LlmConfig {
        LlmConfig {
            base_url: "http://localhost/v1".into(),
            model: "test".into(),
            api_key_file: None,
            api_key_env: None,
            request_timeout_seconds: 1,
        }
    }

    #[tokio::test]
    async fn key_sources_obey_precedence_and_missing_values_fail() {
        let mut llm = config();
        assert_eq!(llm.api_key().await.unwrap(), None);
        let path = std::env::temp_dir().join(format!("flicksy-key-test-{}", std::process::id()));
        tokio::fs::write(&path, " file-key\n").await.unwrap();
        llm.api_key_file = Some(path.clone());
        assert_eq!(llm.api_key().await.unwrap().as_deref(), Some("file-key"));
        llm.api_key_env = Some("PATH".into());
        assert_eq!(
            llm.api_key().await.unwrap().unwrap(),
            std::env::var("PATH").unwrap().trim()
        );
        llm.api_key_env = Some("FLICKSY_TEST_NONEXISTENT_KEY_8D17F".into());
        assert!(llm.api_key().await.is_err());
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[test]
    fn config_accepts_neither_key_source() {
        let config: LlmConfig = toml::from_str(
            "base_url = 'http://localhost/v1'\nmodel = 'test'\nrequest_timeout_seconds = 120",
        )
        .unwrap();
        assert!(config.api_key_file.is_none());
        assert!(config.api_key_env.is_none());
    }
}
