use std::path::{Path, PathBuf};

const LEGACY_DEFAULT: &str = "../../../AxDr_L_Motor/build/host/axdr-host-schema.toml";
const FIRMWARE_SCHEMA: &str = "AxDr_L_Motor/build/host/axdr-host-schema.toml";

fn discover(cwd: &Path, executable: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .chain(executable.parent().into_iter().flat_map(Path::ancestors))
        .map(|root| root.join(FIRMWARE_SCHEMA))
        .find(|path| path.is_file())
        .and_then(|path| path.canonicalize().ok())
}

fn resolve(input: &str, cwd: &Path, executable: &Path) -> Result<PathBuf, String> {
    let input = input.trim();
    if input.is_empty() || input == LEGACY_DEFAULT {
        return discover(cwd, executable)
            .ok_or_else(|| "HostSchema not found. Select the firmware's axdr-host-schema.toml file.".to_owned());
    }
    let path = cwd.join(input);
    path.canonicalize().map_err(|error| format!("Cannot open HostSchema {}: {error}", path.display()))
}

pub fn resolve_schema_path(input: &str) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    resolve(input, &cwd, &executable)
}

#[tauri::command]
pub fn default_schema_path() -> Option<String> {
    resolve_schema_path("").ok().map(|path| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sibling_firmware_from_development_and_release_launch_locations() {
        let root = std::env::temp_dir().join(format!("nmixx-schema-path-{}", std::process::id()));
        let schema = root.join(FIRMWARE_SCHEMA);
        std::fs::create_dir_all(schema.parent().unwrap()).unwrap();
        std::fs::write(&schema, "protocol = \"axdr-canfd\"").unwrap();
        let expected = schema.canonicalize().unwrap();
        let studio = root.join("NMIXX-Motor-Studio");
        let executable = studio.join("target/release/nmixx-motor-studio-desktop");
        for cwd in [&studio, &studio.join("apps/desktop/src-tauri"), &root.join("unrelated") ] {
            assert_eq!(resolve("", cwd, &executable).unwrap(), expected);
            assert_eq!(resolve(LEGACY_DEFAULT, cwd, &executable).unwrap(), expected);
        }
        // An installed executable can discover firmware near the working directory.
        assert_eq!(resolve("", &studio, Path::new("/opt/nmixx/app")).unwrap(), expected);
        // A launch outside the checkout can discover firmware near the executable.
        assert_eq!(resolve("", Path::new("/"), &executable).unwrap(), expected);
        assert_eq!(resolve(expected.to_str().unwrap(), Path::new("/"), &executable).unwrap(), expected);
        // Never substitute a different schema for an explicitly selected missing file.
        assert!(resolve("missing.toml", &root, &executable).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
