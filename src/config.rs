use serde::Deserialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub projects_dir: PathBuf,
    #[serde(default)]
    pub youtube: Option<YouTubeConfig>,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub tracing: TracingConfig,
    #[serde(default)]
    pub operator: OperatorConfig,
    #[allow(dead_code)]
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YouTubeConfig {
    pub client_id: String,
    pub client_secret_file: PathBuf,
    pub encryption_key_file: PathBuf,
    pub redirect_uri: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    pub database: Option<PathBuf>,
    pub secure_cookie: bool,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 3000,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TracingConfig {
    pub level: LogLevel,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn filter(&self) -> &'static str {
        match self {
            Self::Off => "flicksy=off",
            Self::Error => "flicksy=error",
            Self::Warn => "flicksy=warn",
            Self::Info => "flicksy=info",
            Self::Debug => "flicksy=debug",
            Self::Trace => "flicksy=trace",
        }
    }
}

/// Public installation contact, displayed on the privacy and terms pages.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OperatorConfig {
    pub contact_email: String,
}

impl Default for OperatorConfig {
    fn default() -> Self {
        Self {
            contact_email: "operator@example.com".into(),
        }
    }
}

impl OperatorConfig {
    fn validate(&mut self) -> io::Result<()> {
        self.contact_email = self.contact_email.trim().to_owned();
        let valid = self
            .contact_email
            .split_once('@')
            .is_some_and(|(local, domain)| {
                !local.is_empty() && !domain.is_empty() && !domain.contains('@')
            })
            && !self.contact_email.chars().any(|c| {
                c.is_whitespace()
                    || c.is_control()
                    || matches!(c, '?' | '#' | '&' | '<' | '>' | '"' | ':' | ',' | ';')
            });
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "operator.contact_email must be a single email address without mailto parameters",
            ));
        }
        Ok(())
    }

    pub fn contact_href(&self) -> String {
        format!("mailto:{}", self.contact_email)
    }
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
    #[tracing::instrument(skip_all, fields(config_path = %path.display()), err)]
    pub fn load(path: &Path) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let mut config: Self = toml::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        config.operator.validate()?;
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
        if let Some(database) = &mut config.auth.database
            && database.is_relative()
        {
            *database = path.parent().unwrap_or(Path::new(".")).join(&*database);
        }
        if let Some(key_file) = &mut config.llm.api_key_file
            && key_file.is_relative()
        {
            *key_file = path.parent().unwrap_or(Path::new(".")).join(&*key_file);
        }
        if let Some(youtube) = &mut config.youtube {
            let base = path.parent().unwrap_or(Path::new("."));
            if youtube.client_secret_file.is_relative() {
                youtube.client_secret_file = base.join(&youtube.client_secret_file);
            }
            if youtube.encryption_key_file.is_relative() {
                youtube.encryption_key_file = base.join(&youtube.encryption_key_file);
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
    fn tracing_level_defaults_and_rejects_invalid_values() {
        let default: TracingConfig = toml::from_str("").unwrap();
        assert_eq!(default.level.filter(), "flicksy=info");
        let debug: TracingConfig = toml::from_str("level = 'debug'").unwrap();
        assert_eq!(debug.level.filter(), "flicksy=debug");
        assert!(toml::from_str::<TracingConfig>("level = 'verbose'").is_err());
    }

    #[test]
    fn server_defaults_and_configured_values_are_parsed() {
        let defaults: ServerConfig = toml::from_str("").unwrap();
        assert_eq!(defaults.host, "127.0.0.1");
        assert_eq!(defaults.port, 3000);
        let configured: ServerConfig = toml::from_str("host = '0.0.0.0'\nport = 8080").unwrap();
        assert_eq!(configured.host, "0.0.0.0");
        assert_eq!(configured.port, 8080);
        assert!(toml::from_str::<ServerConfig>("port = 70000").is_err());
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

    #[test]
    fn operator_contact_defaults_and_accepts_a_configured_mailbox() {
        let base = "projects_dir='projects'\n[llm]\nbase_url='http://localhost/v1'\nmodel='test'\nrequest_timeout_seconds=10\n";
        let default: Config = toml::from_str(base).unwrap();
        assert_eq!(default.operator.contact_email, "operator@example.com");
        let configured: Config = toml::from_str(&format!(
            "{base}\n[operator]\ncontact_email=' privacy+flicksy@example.org '\n"
        ))
        .unwrap();
        let mut operator = configured.operator;
        operator.validate().unwrap();
        assert_eq!(operator.contact_email, "privacy+flicksy@example.org");
        assert_eq!(
            operator.contact_href(),
            "mailto:privacy+flicksy@example.org"
        );
        assert!(toml::from_str::<OperatorConfig>("contact_emali='test@example.com'").is_err());
    }

    #[test]
    fn invalid_operator_contacts_fail_config_loading() {
        let root =
            std::env::temp_dir().join(format!("flicksy-contact-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        for email in [
            "",
            "not-an-email",
            "@example.com",
            "a@",
            "a@b@c",
            "a b@example.com",
            "a@example.com?bcc=other@example.com",
            "a@example.com#fragment",
            "a@example.com,other@example.com",
        ] {
            std::fs::write(&path, format!(
                "projects_dir='projects'\n[operator]\ncontact_email='{email}'\n[llm]\nbase_url='http://localhost/v1'\nmodel='test'\nrequest_timeout_seconds=10\n"
            )).unwrap();
            assert_eq!(
                Config::load(&path).unwrap_err().kind(),
                io::ErrorKind::InvalidData,
                "{email}"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn example_configs_load_with_placeholder_contacts() {
        for file in ["config.example.toml", "docker/config.toml"] {
            let config = Config::load(Path::new(file)).unwrap();
            assert_eq!(config.operator.contact_email, "operator@example.com");
            assert_eq!(config.llm.model, "YOUR_TOOL_CALLING_MODEL");
        }
    }

    #[test]
    fn youtube_is_optional_and_its_paths_resolve_against_config() {
        let root =
            std::env::temp_dir().join(format!("flicksy-youtube-config-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let base = "projects_dir='projects'\n[llm]\nbase_url='http://localhost'\nmodel='test'\nrequest_timeout_seconds=10\n";
        std::fs::write(&path, base).unwrap();
        assert!(Config::load(&path).unwrap().youtube.is_none());
        std::fs::write(&path, format!("{base}\n[youtube]\nclient_id='client'\nclient_secret_file='secrets/client'\nencryption_key_file='secrets/key'\nredirect_uri='http://localhost/youtube/callback'\n")).unwrap();
        let yt = Config::load(&path).unwrap().youtube.unwrap();
        assert_eq!(yt.client_secret_file, root.join("secrets/client"));
        assert_eq!(yt.encryption_key_file, root.join("secrets/key"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
