use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub uuid: Option<String>,

    #[serde(default)]
    pub recovery_code: Option<String>,

    #[serde(default)]
    pub nickname: Option<String>,
}

pub fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("sh", "tomcat", "bogominer").map(|dirs| dirs.config_dir().join("config.json"))
}

impl Config {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };

        match fs::read_to_string(&path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let data: String = serde_json::to_string_pretty(self).unwrap_or_default();
        let _ = fs::write(&path, data);
    }

    pub fn clear(&mut self) {
        self.uuid = None;
        self.recovery_code = None;
        self.nickname = None;
        self.save();
    }

    pub fn has_credentials(&self) -> bool {
        self.uuid.is_some()
            && self.nickname.is_some()
            && self.recovery_code.as_ref().is_some_and(|s| !s.is_empty())
    }
}
