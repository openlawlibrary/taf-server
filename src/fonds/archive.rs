//! The archive module contains the Archive object for interacting with
//! Fonds Archives, as well as several factory methods.

use crate::fonds::fonds;
use crate::fonds::fonds::Fonds;
use crate::utils::archive::{find_archive_path, get_name_parts};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, create_dir_all, read_to_string, write};
use std::path::{Path, PathBuf};
use toml::ser;

/// The Archive struct is used for interacting with a Fonds Archive.
#[derive(Debug, Clone)]
pub struct Archive {
    /// Path to the Archive
    pub path: PathBuf,
    /// map of auth repo name to Fonds object
    pub fonds_map: HashMap<String, Fonds>,
}

impl Archive {
    /// Get an archive's config object.
    /// # Errors
    /// Will error if unable to find or parse config file at `.taf/config.toml`
    pub fn get_config(&self) -> anyhow::Result<Config> {
        let config_path = &self.path.join(PathBuf::from(".taf/config.toml"));
        let config_str = read_to_string(config_path)?;
        let conf: Config = toml::from_str(&config_str)?;
        Ok(conf)
    }

    /// Get the Archive's root Fonds.
    /// # Errors
    /// Will raise error if unable to find the current root Fonds
    pub fn get_root(&self) -> anyhow::Result<&Fonds> {
        let root = self
            .fonds_map
            .values()
            .find(|fonds| fonds.is_root())
            .ok_or_else(|| anyhow::anyhow!("No root Fonds found in archive"))?;
        Ok(root)
    }

    /// Set the Archive's root Fonds.
    /// # Errors
    /// Will raise error if unable to determine the current
    /// root Fonds.
    pub fn set_root(&mut self, path: Option<PathBuf>) -> anyhow::Result<()> {
        let root: Fonds;
        if let Some(individual_path) = path {
            tracing::info!("Serving individual Fonds at path: {:?}", individual_path);
            root = Fonds::new(&self.path, None, None, Some(individual_path), true)?;
        } else {
            let conf = self.get_config()?;

            let org = conf.root.org;
            let name = conf.root.name;

            tracing::info!("Serving {}/{} at path: {:?}", &org, &name, self.path);

            root = Fonds::new(
                &self.path,
                Some(name),
                Some(org.clone()),
                Some(self.path.clone().join(org)),
                true,
            )?;
        }
        self.fonds_map.insert(root.get_qualified_name(), root);
        Ok(())
    }

    /// Return sorted vector of all Fonds in the Archive.
    #[must_use]
    pub fn get_all_fonds(&self) -> Vec<(String, Fonds)> {
        let mut fonds = self.fonds_map.clone();
        let mut fonds_vec: Vec<(String, Fonds)> = fonds.drain().collect();
        fonds_vec.sort_by(|first_fonds, second_fonds| first_fonds.0.cmp(&second_fonds.0));
        fonds_vec
    }

    /// Parse an Archive.
    /// # Errors
    /// Will raise error if unable to determine the current root fonds or if unable to traverse the child fonds.
    pub fn parse(
        archive_path: PathBuf,
        actual_path: &Path,
        individual: bool,
    ) -> anyhow::Result<Self> {
        let mut archive = Self {
            path: archive_path,
            fonds_map: HashMap::new(),
        };

        let path = if individual {
            actual_path.canonicalize().ok()
        } else {
            None
        };
        archive.set_root(path)?;

        let root = archive.get_root()?;
        let mut visited = vec![root.get_qualified_name()];
        archive.traverse_children(&root.clone(), &mut visited)?;
        Ok(archive)
    }

    /// Traverse the child Fonds of the current Fonds.
    ///
    /// A child Fonds that fails to load is logged and skipped.
    /// # Errors
    /// Will raise error if unable to read the current Fonds's dependencies.
    /// # Panics
    /// If unable to unwrap the parent directory of the current path.
    pub fn traverse_children(
        &mut self,
        current: &Fonds,
        visited: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        if let Some(dependencies) = current.get_dependencies()? {
            for qualified_name in dependencies.sorted_dependencies_names() {
                if visited.contains(&qualified_name) {
                    continue;
                }
                if let Err(err) = self.traverse_child(&qualified_name, visited) {
                    tracing::error!("Skipping Fonds '{qualified_name}': {err:#}");
                }
            }
        }
        Ok(())
    }

    /// Load the child Fonds `qualified_name`, then traverse its children.
    fn traverse_child(
        &mut self,
        qualified_name: &str,
        visited: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        let parent_dir = self.path.clone();
        let (org, name) = get_name_parts(qualified_name)?;
        if fs::metadata(parent_dir.join(&org).join(&name)).is_err() {
            // Fonds does not exist on the filesystem, continue to traverse other Fonds
            return Ok(());
        }
        let child = Fonds::new(
            &self.path,
            Some(name),
            Some(org.clone()),
            Some(parent_dir.join(org)),
            false,
        )?;
        self.fonds_map
            .entry(format!(
                "{org}/{name}",
                org = child.auth_repo.org,
                name = child.auth_repo.name
            ))
            .or_insert_with(|| child.clone());
        visited.push(child.get_qualified_name());
        self.traverse_children(&child, visited)
    }
}

/// Check if the `path` is inside an existing archive
/// # Errors
/// Return an error if the path is inside an existing archive.
fn raise_error_if_in_existing_archive(path: &Path) -> anyhow::Result<bool> {
    let existing_archive_path = find_archive_path(path);
    match existing_archive_path {
        Ok(_) => anyhow::bail!(format!(
            "You cannot create a new archive inside of an existing archive at {} path.",
            path.display()
        )),
        Err(_) => Ok(false),
    }
}

/// Config object for an Archive
#[derive(Deserialize, Serialize)]
pub struct Config {
    /// The root Fonds for this archive
    pub root: fonds::Config,
    /// Whether this is a shallow archive (all repos depth=1)
    pub shallow: bool,
    /// Custom HTTP headers used to interact with the Fonds
    pub headers: Option<Headers>,
}

/// Optional Header configuration for an Archive
#[derive(Default, Deserialize, Serialize)]
pub struct Headers {
    /// Specify a custom header guard to use when requesting a Fonds's current documents.
    pub current_documents_guard: Option<String>,
}

/// Create a new Fonds Archive at path, and return the new archive.
/// # Errors
/// Will error if archive is created inside of an existing archive.
pub fn init(
    path: PathBuf,
    root_name: String,
    root_org: String,
    root_hash: Option<String>,
    shallow: bool,
    headers: Option<Headers>,
) -> anyhow::Result<Box<Archive>> {
    raise_error_if_in_existing_archive(&path)?;
    let fonds_dir = path.join(PathBuf::from("./.taf"));
    create_dir_all(&fonds_dir)?;
    let config_path = fonds_dir.join(PathBuf::from("./config.toml"));
    let conf = Config {
        root: fonds::Config {
            name: root_name,
            org: root_org,
            hash: root_hash,
        },
        shallow,
        headers,
    };
    let conf_str = ser::to_string_pretty(&conf)?;
    write(config_path, conf_str)?;
    let archive = Archive {
        path,
        fonds_map: HashMap::new(),
    };
    Ok(Box::new(archive))
}
