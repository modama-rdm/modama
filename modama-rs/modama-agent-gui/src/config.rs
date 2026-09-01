use std::{
    fs,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use serde::Deserialize;

pub const DEFAULT_AGENT_URL: &str = "http://localhost:3003";
const CONFIG_FILE: &str = "modama-agent-gui.toml";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct GuiConfig {
    pub agent_url: String,
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            agent_url: DEFAULT_AGENT_URL.to_owned(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read configuration {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse configuration {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("the per-user configuration directory is unavailable")]
    UserDirectoryUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigPlatform {
    Unix,
    Windows,
    MacOs,
}

pub fn user_config_path(project_dirs: &ProjectDirs) -> PathBuf {
    project_dirs.config_dir().join(CONFIG_FILE)
}

/// Pure construction of machine-wide candidates, also testable on non-native hosts.
pub fn machine_config_paths(
    platform: ConfigPlatform,
    xdg_config_dirs: Option<&str>,
    program_data: Option<&Path>,
) -> Vec<PathBuf> {
    match platform {
        ConfigPlatform::Unix => xdg_config_dirs
            .filter(|value| !value.is_empty())
            .unwrap_or("/etc/xdg")
            .split(':')
            .filter(|entry| !entry.is_empty())
            .map(|entry| Path::new(entry).join("modama-agent-gui").join(CONFIG_FILE))
            .collect(),
        ConfigPlatform::Windows => program_data
            .map(|root| {
                root.join("modama")
                    .join("modama-agent-gui")
                    .join(CONFIG_FILE)
            })
            .into_iter()
            .collect(),
        ConfigPlatform::MacOs => vec![PathBuf::from(
            "/Library/Application Support/modama.modama-agent-gui/modama-agent-gui.toml",
        )],
    }
}

fn native_machine_config_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let program_data = windows_program_data();
        machine_config_paths(ConfigPlatform::Windows, None, program_data.as_deref())
    }
    #[cfg(target_os = "macos")]
    {
        machine_config_paths(ConfigPlatform::MacOs, None, None)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let xdg = std::env::var("XDG_CONFIG_DIRS").ok();
        machine_config_paths(ConfigPlatform::Unix, xdg.as_deref(), None)
    }
}

#[cfg(target_os = "windows")]
fn windows_program_data() -> Option<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows::Win32::UI::Shell::{CSIDL_COMMON_APPDATA, SHGFP_TYPE_CURRENT, SHGetFolderPathW};

    let mut path = [0u16; 260];
    // SAFETY: `path` is a live, writable MAX_PATH-sized UTF-16 buffer for the
    // duration of the call. The window and token may be NULL. The API writes a
    // NUL-terminated path and does not retain the buffer pointer.
    // SHGetFolderPathW avoids SHGetKnownFolderPath's combase.dll import on Win7.
    // https://learn.microsoft.com/windows/win32/api/shlobj_core/nf-shlobj_core-shgetfolderpathw
    unsafe {
        SHGetFolderPathW(
            None,
            CSIDL_COMMON_APPDATA as i32,
            None,
            SHGFP_TYPE_CURRENT.0 as u32,
            &mut path,
        )
        .ok()?;
    }
    let length = path
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(path.len());
    Some(PathBuf::from(OsString::from_wide(&path[..length])))
}

fn read_config(path: &Path) -> Result<GuiConfig, ConfigError> {
    let source = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_owned(),
        source,
    })?;
    toml::from_str(&source).map_err(|source| ConfigError::Parse {
        path: path.to_owned(),
        source,
    })
}

pub fn load_config(
    explicit_path: Option<&Path>,
    agent_url_override: Option<String>,
) -> Result<GuiConfig, ConfigError> {
    let project_dirs = ProjectDirs::from("", "modama", "modama-agent-gui")
        .ok_or(ConfigError::UserDirectoryUnavailable)?;
    let discovered = std::iter::once(user_config_path(&project_dirs))
        .chain(native_machine_config_paths())
        .collect::<Vec<_>>();
    load_config_from_paths(explicit_path, &discovered, agent_url_override)
}

pub fn load_config_from_paths(
    explicit_path: Option<&Path>,
    discovered_paths: &[PathBuf],
    agent_url_override: Option<String>,
) -> Result<GuiConfig, ConfigError> {
    let mut config = if let Some(path) = explicit_path {
        read_config(path)?
    } else {
        let discovered = discovered_paths.iter().find(|path| path.exists());
        match discovered {
            Some(path) => read_config(path)?,
            None => GuiConfig::default(),
        }
    };
    if let Some(agent_url) = agent_url_override {
        config.agent_url = agent_url;
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_all_platform_machine_paths() {
        assert_eq!(
            machine_config_paths(ConfigPlatform::Unix, Some("/etc/xdg:/opt/config"), None),
            vec![
                PathBuf::from("/etc/xdg/modama-agent-gui/modama-agent-gui.toml"),
                PathBuf::from("/opt/config/modama-agent-gui/modama-agent-gui.toml"),
            ]
        );
        assert_eq!(
            machine_config_paths(
                ConfigPlatform::Windows,
                None,
                Some(Path::new("C:/ProgramData"))
            ),
            vec![PathBuf::from(
                "C:/ProgramData/modama/modama-agent-gui/modama-agent-gui.toml"
            )]
        );
        assert_eq!(
            machine_config_paths(ConfigPlatform::MacOs, None, None),
            vec![PathBuf::from(
                "/Library/Application Support/modama.modama-agent-gui/modama-agent-gui.toml"
            )]
        );
    }

    #[test]
    fn defaults_are_stable() {
        assert_eq!(GuiConfig::default().agent_url, DEFAULT_AGENT_URL);
    }

    #[test]
    fn first_existing_file_wins_and_cli_overrides_it() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("modama-gui-config-{}", std::process::id()));
        let user = root.join("user.toml");
        let machine = root.join("machine.toml");
        fs::create_dir_all(&root)?;
        fs::write(&user, "agent_url = 'http://user:3003'\n")?;
        fs::write(&machine, "agent_url = 'http://machine:3003'\n")?;

        let loaded =
            load_config_from_paths(None, &[user, machine], Some("http://cli:3003".to_owned()))?;
        assert_eq!(loaded.agent_url, "http://cli:3003");
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn malformed_existing_file_is_fatal() -> Result<(), Box<dyn std::error::Error>> {
        let path =
            std::env::temp_dir().join(format!("modama-gui-malformed-{}.toml", std::process::id()));
        fs::write(&path, "agent_url = [not valid")?;
        let result = load_config_from_paths(None, std::slice::from_ref(&path), None);
        assert!(matches!(result, Err(ConfigError::Parse { .. })));
        fs::remove_file(path)?;
        Ok(())
    }
}
