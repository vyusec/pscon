use std::path::PathBuf;

use directories::ProjectDirs;

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("network", "ciatech", "pscon")
}

pub fn data_dir() -> PathBuf {
    if let Ok(home) = std::env::var("PSCON_HOME") {
        return PathBuf::from(home);
    }
    project_dirs()
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".pscon"))
}

pub fn config_path() -> PathBuf {
    if let Ok(path) = std::env::var("PSCON_CONFIG") {
        return PathBuf::from(path);
    }
    project_dirs()
        .map(|p| p.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from(".pscon/config.toml"))
}

pub fn scans_dir() -> PathBuf {
    data_dir().join("scans")
}

pub fn db_path() -> PathBuf {
    data_dir().join("pscon.db")
}

pub fn bridge_token_path() -> PathBuf {
    data_dir().join("bridge-token")
}

pub fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::create_dir_all(scans_dir())?;
    if let Some(parent) = config_path().parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}
