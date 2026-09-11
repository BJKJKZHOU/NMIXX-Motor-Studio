use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::{HostSchema, SchemaError};

#[derive(Debug, Error)]
pub enum SchemaStoreError {
    #[error("cannot determine the default NMIXX schema-store directory")]
    NoDefaultDirectory,
    #[error("schema-store I/O failed: {0}")]
    Io(String),
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error("invalid schema-store key '{0}'")]
    InvalidKey(String),
    #[error("schema '{0}' already exists; import with replace enabled to overwrite it")]
    AlreadyExists(String),
    #[error("schema '{0}' is not present in the local store")]
    NotFound(String),
}

#[derive(Debug, Clone)]
pub struct StoredSchema {
    pub key: String,
    pub path: PathBuf,
    pub schema: HostSchema,
}

#[derive(Debug, Clone)]
pub struct SchemaStore {
    root: PathBuf,
}

impl SchemaStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default() -> Result<Self, SchemaStoreError> {
        Ok(Self::new(default_store_root()?))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn import(&self, source: impl AsRef<Path>, replace: bool) -> Result<StoredSchema, SchemaStoreError> {
        let text = fs::read_to_string(source.as_ref())
            .map_err(|error| SchemaStoreError::Io(error.to_string()))?;
        let schema = HostSchema::parse(&text)?;
        let key = schema_store_key(&schema)?;
        let path = self.path_for_key(&key)?;

        if path.exists() && !replace {
            return Err(SchemaStoreError::AlreadyExists(key));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| SchemaStoreError::Io(error.to_string()))?;
        }
        fs::write(&path, text).map_err(|error| SchemaStoreError::Io(error.to_string()))?;

        Ok(StoredSchema { key, path, schema })
    }

    pub fn list(&self) -> Result<Vec<StoredSchema>, SchemaStoreError> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut stored = Vec::new();
        for repository in fs::read_dir(&self.root).map_err(|error| SchemaStoreError::Io(error.to_string()))? {
            let repository = repository.map_err(|error| SchemaStoreError::Io(error.to_string()))?;
            if !repository.file_type().map_err(|error| SchemaStoreError::Io(error.to_string()))?.is_dir() {
                continue;
            }
            let repository_name = repository.file_name().to_string_lossy().into_owned();
            for file in fs::read_dir(repository.path()).map_err(|error| SchemaStoreError::Io(error.to_string()))? {
                let file = file.map_err(|error| SchemaStoreError::Io(error.to_string()))?;
                let path = file.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                let key = format!("{repository_name}/{stem}");
                match HostSchema::load(&path) {
                    Ok(schema) => stored.push(StoredSchema { key, path, schema }),
                    Err(error) => return Err(SchemaStoreError::Schema(error)),
                }
            }
        }
        stored.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(stored)
    }

    pub fn get(&self, key: &str) -> Result<StoredSchema, SchemaStoreError> {
        let path = self.path_for_key(key)?;
        if !path.exists() {
            return Err(SchemaStoreError::NotFound(key.to_owned()));
        }
        let schema = HostSchema::load(&path)?;
        Ok(StoredSchema { key: key.to_owned(), path, schema })
    }

    pub fn export(&self, key: &str, destination: impl AsRef<Path>) -> Result<(), SchemaStoreError> {
        let stored = self.get(key)?;
        if let Some(parent) = destination.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|error| SchemaStoreError::Io(error.to_string()))?;
            }
        }
        fs::copy(&stored.path, destination.as_ref())
            .map_err(|error| SchemaStoreError::Io(error.to_string()))?;
        Ok(())
    }

    fn path_for_key(&self, key: &str) -> Result<PathBuf, SchemaStoreError> {
        let (repository, revision) = key
            .split_once('/')
            .ok_or_else(|| SchemaStoreError::InvalidKey(key.to_owned()))?;
        if !valid_segment(repository) || !valid_segment(revision) {
            return Err(SchemaStoreError::InvalidKey(key.to_owned()));
        }
        Ok(self.root.join(repository).join(format!("{revision}.toml")))
    }
}

pub fn schema_store_key(schema: &HostSchema) -> Result<String, SchemaStoreError> {
    let repository = sanitize_segment(&schema.source.repository)?;
    let revision = sanitize_segment(&schema.source.git_sha)?;
    Ok(format!("{repository}/{revision}"))
}

fn sanitize_segment(value: &str) -> Result<String, SchemaStoreError> {
    if valid_segment(value) {
        return Ok(value.to_owned());
    }
    Err(SchemaStoreError::InvalidKey(value.to_owned()))
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn default_store_root() -> Result<PathBuf, SchemaStoreError> {
    #[cfg(target_os = "windows")]
    {
        if let Some(root) = env::var_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(root).join("NMIXX Motor Studio").join("schemas"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = env::var_os("HOME") {
            return Ok(PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("NMIXX Motor Studio")
                .join("schemas"));
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Some(root) = env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(root).join("nmixx-motor-studio").join("schemas"));
        }
        if let Some(home) = env::var_os("HOME") {
            return Ok(PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("nmixx-motor-studio")
                .join("schemas"));
        }
    }

    Err(SchemaStoreError::NoDefaultDirectory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_keys() {
        let store = SchemaStore::new("/tmp/nmixx-test-store");
        assert!(matches!(store.path_for_key("../escape"), Err(SchemaStoreError::InvalidKey(_))));
        assert!(matches!(store.path_for_key("repo/a/b"), Err(SchemaStoreError::InvalidKey(_))));
    }

    #[test]
    fn builds_stable_key_from_schema_source() {
        let schema = HostSchema::parse(
            r#"
schema_version = 1
protocol = "axdr-canfd-v1"
[source]
repository = "AxDr_L_Motor"
git_sha = "abc123"
parameter_schema = 1
"#,
        )
        .unwrap();
        assert_eq!(schema_store_key(&schema).unwrap(), "AxDr_L_Motor/abc123");
    }
}
