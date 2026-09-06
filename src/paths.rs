use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

const APP: &str = "flint";
const LEGACY: &str = "rayblast";
const LOGO_MARK: &[u8] = include_bytes!("../share/flint-mark.png");
const LOGO_APP: &[u8] = include_bytes!("../share/flint.png");

pub fn data_dir() -> PathBuf {
    if let Some(dir) = env_dir("FLINT_DATA_DIR") {
        return dir;
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP)
}

pub fn config_dir() -> PathBuf {
    if let Some(dir) = env_dir("FLINT_CONFIG_DIR") {
        return dir;
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP)
}

fn env_dir(key: &str) -> Option<PathBuf> {
    let raw = std::env::var_os(key)?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

pub fn runtime_dir() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP)
}

pub fn logo_mark() -> PathBuf {
    data_dir().join("flint-mark.png")
}

pub fn logo_app() -> PathBuf {
    data_dir().join("flint.png")
}

pub fn ensure() {
    ensure_dir(&data_dir());
    ensure_dir(&config_dir());
    ensure_dir(&runtime_dir());
    migrate_legacy();
    tighten_private_file(&config_dir().join("config.json"));
    tighten_private_file(&config_dir().join("auth.json"));
    tighten_private_file(&config_dir().join("api-keys.json"));
    tighten_private_file(&config_dir().join("snippets.json"));
    tighten_private_file(&config_dir().join("aliases.json"));
    tighten_private_file(&config_dir().join("quicklinks.json"));
    tighten_private_file(&config_dir().join("layouts.json"));
    tighten_private_file(&config_dir().join("quit-keep.json"));
    tighten_private_file(&data_dir().join("notes.json"));
    tighten_private_file(&data_dir().join("clipboard.json"));
    tighten_private_file(&data_dir().join("usage.json"));
    tighten_private_file(&data_dir().join("favorites.json"));
    tighten_private_file(&data_dir().join("calc-history.json"));
    let _ = crate::db::open();
    tighten_private_file(&data_dir().join("flint.db"));
    let _ = write_private(&logo_mark(), LOGO_MARK);
    let _ = write_private(&logo_app(), LOGO_APP);
}

pub fn ensure_dir(dir: &Path) {
    let _ = fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(dir);
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
}

pub fn write_private(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent);
    }
    fs::write(path, contents)?;
    tighten_private_file(path);
    Ok(())
}

pub fn tighten_private_file(path: &Path) {
    if path.exists() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

fn migrate_legacy() {
    migrate_file("config.json", true);
    migrate_file("snippets.json", true);
    migrate_file("auth.json", true);
    migrate_file("notes.json", false);
    migrate_file("clipboard.json", false);
    migrate_file("usage.json", false);
}

fn migrate_file(name: &str, config: bool) {
    let dest_dir = if config { config_dir() } else { data_dir() };
    let dest = dest_dir.join(name);
    if dest.exists() {
        return;
    }
    let src_parent = if config {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(LEGACY)
    } else {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(LEGACY)
    };
    let src = src_parent.join(name);
    if src.exists() {
        let _ = fs::copy(&src, &dest);
        tighten_private_file(&dest);
    }
}

#[cfg(test)]
mod tests {
    use super::APP;

    #[test]
    fn app_dir_is_flint() {
        assert_eq!(APP, "flint");
    }
}
