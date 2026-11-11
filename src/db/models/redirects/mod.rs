use std::collections::HashSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod manager;

/// Trait for managing URL redirects.
#[async_trait]
pub trait Manager {
    /// Finds a redirect target for a given URL.
    async fn find_redirect_for_url(
        &self,
        fonds: String,
        repo_name: String,
        from_url: String,
    ) -> anyhow::Result<Option<String>>;

    /// Returns the set of `(fonds_name, repo_name)` pairs that currently have
    /// at least one redirect configured.
    async fn repos_with_redirects(&self) -> anyhow::Result<HashSet<(String, String)>>;
}

/// Trait for managing redirects within a transactional context.
#[async_trait]
pub trait TxManager {
    /// Inserts multiple redirect mappings in a single transactional operation.
    async fn insert_bulk(
        &mut self,
        fonds: &str,
        repo_name: &str,
        redirect_pairs: Vec<RedirectPair>,
    ) -> anyhow::Result<()>;
    /// Deletes all redirects of a fonds.
    async fn delete_for_fonds(&mut self, fonds: &str) -> anyhow::Result<()>;
}

#[derive(sqlx::FromRow, Deserialize, Serialize, Debug)]
/// Model for redirects.
pub struct RedirectPair {
    /// Source URL that should be redirected.
    /// Represents the original request path that triggers the redirect.
    pub from_url: String,
    /// Destination URL where the request should be redirected.
    /// This is the target path that replaces the original URL.
    pub to_url: String,
}

impl RedirectPair {
    /// Create a new redirect pairs.
    #[must_use]
    pub const fn new(from_url: String, to_url: String) -> Self {
        Self { from_url, to_url }
    }
}
