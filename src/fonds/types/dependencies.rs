//! A Fonds's dependencies.
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;

/// A map of Fonds names to their dependencies.
#[derive(Serialize, Deserialize, Debug)]
pub struct Dependencies {
    /// An inner map of Fonds keys to their dependencies.
    pub dependencies: HashMap<String, Dependency>,
}

/// A single dependency as specified in a Fonds's `dependencies.json` file.
///
/// Missing fields default to empty, so that one incomplete entry does not stop
/// the other Fonds from loading; `taf-server check` reports empty fields.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dependency {
    /// The out-of-band authenticated hash of the Fonds.
    #[serde(rename = "out-of-band-authentication", default)]
    pub out_of_band_authentication: String,
    /// The default branch for a Fonds.
    #[serde(default)]
    pub branch: String,
}

impl Dependencies {
    /// Get the dependencies names for a given Fonds.
    #[must_use]
    pub fn sorted_dependencies_names(&self) -> Vec<String> {
        let mut keys = self.dependencies.keys().cloned().collect::<Vec<String>>();
        keys.sort();
        keys
    }
}
