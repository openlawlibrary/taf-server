//! A Fonds's data repositories.
use std::{collections::HashMap, fmt, string::String};

use anyhow::Context as _;
use regex::Regex;
use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize, Deserializer,
};
use serde_derive::Serialize;
use serde_json::Value;

use crate::utils::archive::get_name_parts;

/// Repositories object
///
/// Represents data repositories in a Fonds.
/// Repositories object is serialized from `repositories.json`.
///
/// `repositories.json` is expected to exist in /targets/repositories.json in the authentication repository.
/// # Examples
///
/// ```rust
/// use serde_json::json;
/// use taf_server::fonds::types::repositories::Repositories;
///
/// let data = r#"
/// {
///     "scopes": ["some/scope/path"],
///     "repositories": {
///         "test_org_1/data_repo_1": {
///             "custom": {
///                 "serve": "latest",
///                 "routes": ["example-route-glob-pattern-1"]
///             }
///         },
///         "test_org_1/data_repo_2": {
///             "custom": {
///                 "serve": "latest",
///                 "serve-prefix": "_prefix",
///                 "is_fallback": true
///             }
///         }
///     }
/// }
/// "#;
/// let repositories: Repositories = serde_json::from_str(data).unwrap();
/// assert_eq!(repositories.scopes.unwrap(), vec!["some/scope/path"]);
/// assert!(repositories.repositories.contains_key("test_org_1/data_repo_1"));
/// assert!(repositories.repositories.contains_key("test_org_1/data_repo_2"));
/// ```
#[derive(Debug, Clone, Serialize, Default)]
pub struct Repositories {
    /// Scopes of the repositories
    pub scopes: Option<Vec<String>>,
    /// Map of repositories. The key is the name of the repository.
    pub repositories: HashMap<String, Repository>,
}

/// Repository object
///
/// Represents one concrete data repository in a fonds.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Repository {
    /// Fully qualified name in `<org>/<name>` format.
    /// This is the key of the `repositories` entries.
    pub name: String,
    /// Custom object
    pub custom: Custom,
}

impl Repository {
    /// Get the org of the repository.
    /// The org is the first part of the name, before the `/`.
    #[must_use]
    pub fn get_org(&self) -> String {
        self.name.split('/').next().unwrap_or_default().to_owned()
    }

    /// Get the name of the repository.
    /// The name is the second part of the name, after the `/`.
    #[must_use]
    pub fn get_name(&self) -> String {
        self.name.split('/').nth(1).unwrap_or_default().to_owned()
    }

    /// Get the type of the repository.
    #[must_use]
    pub fn get_type(&self) -> Option<String> {
        self.custom.repository_type.clone()
    }

    /// Returns true if this repository is marked as archived.
    #[must_use]
    pub fn is_archived(&self) -> bool {
        self.custom.archived.unwrap_or(false)
    }

    /// Returns true if this repository has a preview URL configured.
    #[must_use]
    pub const fn has_preview(&self) -> bool {
        self.custom.preview.is_some()
    }
}

/// Custom object
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Custom {
    #[serde(rename = "type")]
    /// Type of data repository. e.g. `rdf`, `html`, `pdf`, `xml`, or any other.
    pub repository_type: Option<String>,
    /// "latest" or "historical". Currently not used by the framework.
    pub serve: String,
    /// Vector of glob patterns used by the Actix framework to resolve url routing.
    /// Routing to use when locating current blobs from the data repository.
    /// Example:
    ///
    /// Given a `["_underscore/.*"]` glob pattern, the following urls are expected to be routed to the current data repository:
    ///
    /// - `/_underscore/`
    /// - `/_underscore/any/path`
    /// - `/_underscore/any/path/with/any/number/of/segments`
    pub routes: Option<Vec<String>>,
    #[serde(rename = "serve-prefix")]
    /// Prefix to use when serving the data repository.
    /// If `None`, the data repository will be served at the root.
    /// If `Some("prefix")`, the data repository will be served from `/prefix/<data>`.
    pub scope: Option<String>,
    /// Whether the data repository is a fallback.
    ///
    /// When a data repository is a fallback, it is used to serve current blobs when no other data repository matches the request.
    pub is_fallback: Option<bool>,
    /// Whether this data repository is archived (i.e. superseded by a newer repository of the same type).
    ///
    /// When `true`, this repository held publications built before a migration to a newer repository.
    pub archived: Option<bool>,
    /// Git URL of the preview repository associated with this data repository.
    ///
    /// When set, this repository has a corresponding preview repository used for non-production environments.
    pub preview: Option<String>,
}

impl Repositories {
    /// Check that every repository name is in `<org>/<name>` format and that
    /// every route compiles as the regex Actix builds from it.
    ///
    /// # Errors
    /// Will error on the first invalid repository name or route.
    pub fn validate(&self) -> anyhow::Result<()> {
        self.repositories.values().try_for_each(|repository| {
            get_name_parts(&repository.name)?;
            for route in repository.custom.routes.iter().flatten() {
                Regex::new(&format!("^/(?P<tail>{route})$"))
                    .with_context(|| format!("invalid route '{route}' in '{}'", repository.name))?;
            }
            Ok(())
        })
    }

    /// Get the repositories sorted by the length of their routes, longest first.
    ///
    /// This is needed for serving current documents because Actix routes are matched in the order they are added.
    #[must_use]
    #[expect(
        clippy::iter_over_hash_type,
        reason = "Returns sorted repositories, even with iterating over hash type"
    )]
    pub fn get_sorted(&self) -> Vec<&Repository> {
        let mut result = Vec::new();
        for repository in self.repositories.values() {
            result.push(repository);
        }
        result.sort_by(|repo1, repo2| {
            let routes1 = repo1.custom.routes.as_ref().map_or(0, |routes| {
                routes.iter().map(String::len).max().unwrap_or(0)
            });
            let routes2 = repo2.custom.routes.as_ref().map_or(0, |routes| {
                routes.iter().map(String::len).max().unwrap_or(0)
            });
            routes2.cmp(&routes1)
        });
        result
    }

    /// Get all `Repository`.
    #[must_use]
    #[expect(
        clippy::iter_over_hash_type,
        reason = "Returns repositories, even with iterating over hash type"
    )]
    pub fn get_all(&self) -> Vec<&Repository> {
        let mut result = Vec::new();
        for repository in self.repositories.values() {
            result.push(repository);
        }
        result
    }

    /// Filter and return a `Repository` by it's custom type.
    #[must_use]
    pub fn get_one_by_custom_type(&self, repository_type: &str) -> Option<&Repository> {
        self.repositories.values().find(|repository| {
            repository.custom.repository_type.as_deref() == Some(repository_type)
        })
    }

    /// Filter and return a `Repository` by it's serve type.
    #[must_use]
    pub fn get_all_by_custom_type(&self, repository_type: &str) -> Vec<&Repository> {
        self.repositories
            .values()
            .filter(|repository| {
                repository.custom.repository_type.as_deref() == Some(repository_type)
            })
            .collect()
    }

    /// Filter and return all `Repository` objects by it's serve type.
    ///
    /// Example:
    /// ```rust
    /// use serde_json::json;
    /// use taf_server::fonds::types::repositories::Repositories;
    ///
    /// let data = r#"
    /// {
    ///     "scopes": ["some/scope/path"],
    ///    "repositories": {
    ///        "test_org_1/data_repo_1": {
    ///           "custom": {
    ///          "serve": "latest",
    ///         "routes": ["example-route-glob-pattern-1"]
    ///        }
    ///   },
    ///  "test_org_1/data_repo_2": {
    ///    "custom": {
    ///      "serve": "latest",
    ///      "serve-prefix": "_prefix",
    ///      "is_fallback": true
    ///     }
    ///   }
    ///  }
    /// }
    /// "#;
    /// let repositories: Repositories = serde_json::from_str(data).unwrap();
    /// let serve_type = "latest";
    /// let repos = repositories.get_all_by_serve_type(serve_type);
    /// assert_eq!(repos.len(), 2);
    /// ```
    #[must_use]
    pub fn get_all_by_serve_type(&self, serve_type: &str) -> Vec<&Repository> {
        self.repositories
            .values()
            .filter(|repository| repository.custom.serve == serve_type)
            .collect()
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Use serde default trait implementations"
)]
impl<'de> Deserialize<'de> for Repositories {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        /// Visitor for the Repositories struct
        struct RepositoriesVisitor;

        impl<'de> Visitor<'de> for RepositoriesVisitor {
            type Value = Repositories;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Repositories")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Repositories, V::Error>
            where
                V: MapAccess<'de>,
            {
                Self::deserialize_repositories(&mut map)
            }
        }

        impl RepositoriesVisitor {
            /// Deserialize the repositories map from the `repositories.json` file.
            fn deserialize_repositories<'de, V>(map: &mut V) -> Result<Repositories, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut scopes = None;
                let mut repositories = HashMap::new();
                while let Some(key) = map.next_key()? {
                    match key {
                        "scopes" => {
                            scopes = map.next_value()?;
                        }
                        "repositories" => {
                            repositories = Self::deserialize_repositories_values(map)?;
                        }
                        _ => {
                            return Err(de::Error::unknown_field(key, &["scopes", "repositories"]))
                        }
                    }
                }
                Ok(Repositories {
                    scopes,
                    repositories,
                })
            }

            /// Deserialize individual repositories from the `repositories.json` file.
            fn deserialize_repositories_values<'de, V>(
                map: &mut V,
            ) -> Result<HashMap<String, Repository>, V::Error>
            where
                V: MapAccess<'de>,
            {
                let repositories_json: HashMap<String, Value> = map.next_value()?;
                let mut keys = repositories_json.keys().clone().collect::<Vec<_>>();
                keys.sort();
                let mut repositories = HashMap::new();
                for key in keys {
                    let custom_value = repositories_json
                        .get(key)
                        .ok_or_else(|| de::Error::custom(format!("Missing {key} in JSON")))?
                        .get("custom")
                        .ok_or_else(|| de::Error::custom("Missing 'custom' field"))?;
                    let custom: Custom =
                        serde_json::from_value(custom_value.clone()).map_err(|err| {
                            de::Error::custom(format!("Failed to deserialize 'custom': {err}"))
                        })?;
                    let repo = Repository {
                        name: key.clone(),
                        custom,
                    };
                    repositories.insert(key.clone(), repo);
                }
                Ok(repositories)
            }
        }
        /// Expected fields in the `repositories.json` file.
        const FIELDS: &[&str] = &["scopes", "repositories"];
        deserializer.deserialize_struct("Repositories", FIELDS, RepositoriesVisitor)
    }
}
