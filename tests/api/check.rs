use std::path::Path;

use anyhow::Result;
use taf_server::fonds::types::repositories::Repositories;
use taf_server::fonds::types::targets_metadata::TargetsMetadata;
use taf_server::server::errors::CliError;
use taf_server::utils::check;

use crate::archive_testtools::{
    self,
    config::{get_basic_test_data_repositories, ArchiveType, Jurisdiction},
    write_to_file, GitRepository,
};
use crate::common;

/// Names of the six data repositories created by
/// `get_basic_test_data_repositories`, used to patch in valid target files
/// for tests that need a clean baseline.
const BASIC_REPO_NAMES: [&str; 6] = [
    "law-html",
    "law-rdf",
    "law-xml",
    "law-xml-codified",
    "law-pdf",
    "law-other",
];

/// Make the root Fonds of a basic archive (created via
/// `initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single))`)
/// fully valid: scopes set, a valid `info.json`, and a valid target file
/// (with all fields) for every default data repository except those listed
/// in `skip_target_files`.
fn make_root_valid(archive_path: &Path, org: &str, skip_target_files: &[&str]) -> Result<()> {
    let org_path = archive_path.join(org);
    let repo_path = org_path.join("law");

    archive_testtools::init_auth_repository(
        &org_path,
        org,
        &get_basic_test_data_repositories()?,
        Some(&vec!["us/example".to_string()]),
        None,
    )?;
    archive_testtools::add_info_json(&repo_path, org, "law")?;

    for name in BASIC_REPO_NAMES.iter().copied() {
        if skip_target_files.contains(&name) {
            continue;
        }
        archive_testtools::add_target_file(
            &repo_path,
            org,
            name,
            &TargetsMetadata {
                branch: "main".into(),
                commit: "abc123".into(),
                build_date: Some("2024-01-01".into()),
                codified_date: Some("2024-01-01".into()),
            },
        )?;
    }
    Ok(())
}

/// Initialize a Fonds with an empty (but valid) `targets/repositories.json`,
/// a valid `info.json`, and no `targets/dependencies.json`. Used as a
/// dependency target that should recurse cleanly (0 errors, at most a "no
/// scopes" warning).
fn init_minimal_valid_fonds(archive_path: &Path, org: &str) -> Result<()> {
    let repo_path = archive_path.join(org).join("law");
    std::fs::create_dir_all(&repo_path)?;
    let repo = GitRepository::init(&repo_path)?;

    let repositories = Repositories::default();
    let content = serde_json::to_string_pretty(&repositories)?;
    repo.add_file(&repo_path.join("targets"), "repositories.json", &content)?;
    repo.commit(Some("targets/repositories.json"), "Add repositories.json")?;

    archive_testtools::add_info_json(&repo_path, org, "law")?;
    Ok(())
}

/// Initialize a Fonds's auth repo with a valid `info.json` but no
/// `targets/repositories.json` at all.
fn init_fonds_without_repositories_json(archive_path: &Path, org: &str) -> Result<()> {
    let repo_path = archive_path.join(org).join("law");
    std::fs::create_dir_all(&repo_path)?;
    GitRepository::init(&repo_path)?;
    archive_testtools::add_info_json(&repo_path, org, "law")?;
    Ok(())
}

#[test]
fn test_check_when_archive_valid_expect_success() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();
    assert!(
        report.errors.is_empty(),
        "unexpected errors: {:?}",
        report.errors
    );
    assert!(
        report.warnings.is_empty(),
        "unexpected warnings: {:?}",
        report.warnings
    );

    let result = check::run(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    );
    assert!(result.is_ok(), "expected Ok(()), got {result:?}");
}

#[test]
fn test_check_when_repositories_json_missing_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();

    init_fonds_without_repositories_json(archive_path.path(), "ghost_org").unwrap();
    archive_testtools::add_dependencies(archive_path.path(), "test_org", vec!["ghost_org"], None)
        .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert_eq!(report.errors[0].fonds, "ghost_org/law");
    assert!(report.errors[0]
        .message
        .contains("required file is missing"));

    let result = check::run(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    );
    assert!(
        matches!(result, Err(CliError::CheckFailed)),
        "got {result:?}"
    );
}

/// `repositories.json` and `dependencies.json` are both read during Fonds
/// construction (`Fonds::new` / `Archive::traverse_children`), so a
/// syntactically malformed one anywhere in the tree makes `Archive::parse`
/// fail before `check_fonds` ever runs. This collapses to one generic
/// error rather than the per-fonds "invalid JSON" message
/// `check_repositories_json` would give for a file that's valid JSON but
/// violates a business rule.
#[test]
fn test_check_when_repositories_json_malformed_expect_generic_parse_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");

    write_to_file(
        &auth_repo_path,
        "{ not valid json".to_string(),
        "repositories.json".to_string(),
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert_eq!(report.errors[0].fonds, "None");
    assert!(report.errors[0].message.contains("failed to parse archive"));
}

/// `type` is currently downgraded to a warning (see the `IMPORTANT` note in
/// `check_repositories_consistency`) and no longer exempts repositories
/// whose name ends in `docs` -- update this test (and reintroduce a
/// docs-exemption case) once that flips back to an error.
#[test]
fn test_check_when_type_missing_expect_warning() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");
    archive_testtools::add_info_json(&auth_repo_path, "test_org", "law").unwrap();

    let content = r#"
    {
        "repositories": {
            "test_org/law-html": {
                "custom": { "serve": "latest", "routes": [".*"] }
            }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "repositories.json".to_string()).unwrap();
    archive_testtools::add_target_file(
        &auth_repo_path,
        "test_org",
        "law-html",
        &TargetsMetadata {
            branch: "main".into(),
            commit: "abc123".into(),
            build_date: None,
            codified_date: None,
        },
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert!(
        report.errors.is_empty(),
        "unexpected errors: {:?}",
        report.errors
    );
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.message.contains("missing required field 'type'")));
}

/// Per review this whole check is slated for removal ("because of some
/// internal setup, let's omit this check entirely"). Delete this test when
/// that lands.
#[test]
fn test_check_when_serve_prefix_and_routes_missing_expect_warning() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");
    archive_testtools::add_info_json(&auth_repo_path, "test_org", "law").unwrap();

    let content = r#"
    {
        "repositories": {
            "test_org/law-html": {
                "custom": { "type": "html", "serve": "latest" }
            }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "repositories.json".to_string()).unwrap();
    archive_testtools::add_target_file(
        &auth_repo_path,
        "test_org",
        "law-html",
        &TargetsMetadata {
            branch: "main".into(),
            commit: "abc123".into(),
            build_date: None,
            codified_date: None,
        },
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert!(
        report.errors.is_empty(),
        "unexpected errors: {:?}",
        report.errors
    );
    assert!(report.warnings.iter().any(|warning| warning
        .message
        .contains("must have either 'serve-prefix' or 'routes'")));
}

#[test]
fn test_check_when_serve_value_invalid_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");
    archive_testtools::add_info_json(&auth_repo_path, "test_org", "law").unwrap();

    let content = r#"
    {
        "repositories": {
            "test_org/law-html": {
                "custom": { "type": "html", "serve": "sometimes", "routes": [".*"] }
            }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "repositories.json".to_string()).unwrap();
    archive_testtools::add_target_file(
        &auth_repo_path,
        "test_org",
        "law-html",
        &TargetsMetadata {
            branch: "main".into(),
            commit: "abc123".into(),
            build_date: None,
            codified_date: None,
        },
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert!(report.errors[0].message.contains("invalid 'serve' value"));
}

#[test]
fn test_check_when_serve_prefix_duplicated_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");
    archive_testtools::add_info_json(&auth_repo_path, "test_org", "law").unwrap();

    let content = r#"
    {
        "repositories": {
            "test_org/law-xml": {
                "custom": { "type": "xml", "serve": "latest", "serve-prefix": "_xml" }
            },
            "test_org/law-xml-codified": {
                "custom": { "type": "xml", "serve": "latest", "serve-prefix": "_xml" }
            }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "repositories.json".to_string()).unwrap();

    let metadata = TargetsMetadata {
        branch: "main".into(),
        commit: "abc123".into(),
        build_date: None,
        codified_date: None,
    };
    archive_testtools::add_target_file(&auth_repo_path, "test_org", "law-xml", &metadata).unwrap();
    archive_testtools::add_target_file(&auth_repo_path, "test_org", "law-xml-codified", &metadata)
        .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert!(report.errors[0]
        .message
        .contains("duplicate 'serve-prefix'"));
    assert!(report.errors[0].message.contains("law-xml"));
    assert!(report.errors[0].message.contains("law-xml-codified"));
}

#[test]
fn test_check_when_multiple_fallbacks_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");
    archive_testtools::add_info_json(&auth_repo_path, "test_org", "law").unwrap();

    let content = r#"
    {
        "repositories": {
            "test_org/law-pdf": {
                "custom": { "type": "pdf", "serve": "latest", "routes": [".*\\.pdf"], "is_fallback": true }
            },
            "test_org/law-other": {
                "custom": { "type": "other", "serve": "latest", "routes": [".*"], "is_fallback": true }
            }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "repositories.json".to_string()).unwrap();

    let metadata = TargetsMetadata {
        branch: "main".into(),
        commit: "abc123".into(),
        build_date: None,
        codified_date: None,
    };
    archive_testtools::add_target_file(&auth_repo_path, "test_org", "law-pdf", &metadata).unwrap();
    archive_testtools::add_target_file(&auth_repo_path, "test_org", "law-other", &metadata)
        .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert!(report.errors[0]
        .message
        .contains("multiple repositories marked"));
}

#[test]
fn test_check_when_data_repository_missing_or_not_git_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");
    archive_testtools::add_info_json(&auth_repo_path, "test_org", "law").unwrap();

    std::fs::create_dir_all(archive_path.path().join("test_org/not-a-repo")).unwrap();

    let content = r#"
    {
        "repositories": {
            "test_org/ghost-repo": {
                "custom": { "type": "html", "serve": "latest", "routes": [".*"] }
            },
            "test_org/not-a-repo": {
                "custom": { "type": "html", "serve": "latest", "routes": [".*"] }
            }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "repositories.json".to_string()).unwrap();

    let metadata = TargetsMetadata {
        branch: "main".into(),
        commit: "abc123".into(),
        build_date: None,
        codified_date: None,
    };
    archive_testtools::add_target_file(&auth_repo_path, "test_org", "ghost-repo", &metadata)
        .unwrap();
    archive_testtools::add_target_file(&auth_repo_path, "test_org", "not-a-repo", &metadata)
        .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 2, "errors: {:?}", report.errors);
    assert!(report
        .errors
        .iter()
        .any(|error| error.message.contains("does not exist at")));
    assert!(report
        .errors
        .iter()
        .any(|error| error.message.contains("is not a valid git repository")));
}

#[test]
fn test_check_when_archived_data_repository_missing_expect_warning() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");
    archive_testtools::add_info_json(&auth_repo_path, "test_org", "law").unwrap();

    let content = r#"
    {
        "scopes": ["us/example"],
        "repositories": {
            "test_org/law-archived": {
                "custom": { "type": "html", "serve": "historical", "routes": [".*"], "archived": true }
            }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "repositories.json".to_string()).unwrap();

    archive_testtools::add_target_file(
        &auth_repo_path,
        "test_org",
        "law-archived",
        &TargetsMetadata {
            branch: "main".into(),
            commit: "abc123".into(),
            build_date: Some("2024-01-01".into()),
            codified_date: Some("2024-01-01".into()),
        },
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert!(
        report.errors.is_empty(),
        "unexpected errors: {:?}",
        report.errors
    );
    assert_eq!(report.warnings.len(), 1, "warnings: {:?}", report.warnings);
    assert!(report.warnings[0].message.contains("does not exist at"));
}

/// Same collapse as the `repositories.json` malformed test -- see its comment.
#[test]
fn test_check_when_dependencies_json_malformed_expect_generic_parse_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");

    write_to_file(
        &auth_repo_path,
        "{ not valid json".to_string(),
        "dependencies.json".to_string(),
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert_eq!(report.errors[0].fonds, "None");
    assert!(report.errors[0].message.contains("failed to parse archive"));
}

#[test]
fn test_check_when_dependency_fields_empty_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");

    init_minimal_valid_fonds(archive_path.path(), "ghost_org").unwrap();
    init_minimal_valid_fonds(archive_path.path(), "another_org").unwrap();

    let content = r#"
    {
        "dependencies": {
            "ghost_org/law": { "out-of-band-authentication": "", "branch": "main" },
            "another_org/law": { "out-of-band-authentication": "abc123", "branch": "" }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "dependencies.json".to_string()).unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 2, "errors: {:?}", report.errors);
    assert!(report
        .errors
        .iter()
        .any(|error| error.message.contains("out-of-band-authentication")));
    assert!(report
        .errors
        .iter()
        .any(|error| error.message.contains("non-empty 'branch'")));
}

#[test]
fn test_check_when_dependencies_has_duplicate_key_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    init_minimal_valid_fonds(archive_path.path(), "ghost_org").unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");

    // Syntactically valid JSON, but "ghost_org/law" appears twice. This
    // parses fine into `Dependencies` (a HashMap silently keeps the last
    // occurrence), so it specifically exercises the raw-text duplicate
    // check rather than any typed-struct-level validation.
    let content = r#"
    {
        "dependencies": {
            "ghost_org/law": { "out-of-band-authentication": "abc123", "branch": "main" },
            "ghost_org/law": { "out-of-band-authentication": "def456", "branch": "master" }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "dependencies.json".to_string()).unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert!(report.errors[0]
        .message
        .contains("duplicate dependencies found"));
    assert!(report.errors[0].message.contains("ghost_org/law"));
}

/// A direct self-reference is both a business-rule violation (caught by
/// `check_dependencies_consistency`'s self-reference check) and a 1-node
/// cycle (caught when recursing into it finds its own name still on the
/// `depth` stack), so expect 2 errors, not 1.
#[test]
fn test_check_when_dependency_self_reference_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");

    let content = r#"
    {
        "dependencies": {
            "test_org/law": { "out-of-band-authentication": "abc123", "branch": "main" }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "dependencies.json".to_string()).unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    // The explicit "lists itself" consistency check was removed in favor of
    // depth-based cycle detection, which catches a self-reference as a
    // 1-node cycle -- so exactly 1 error now, not 2.
    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert!(report.errors[0].message.contains("cycle"));
}

#[test]
fn test_check_when_dependency_directory_missing_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");

    let content = r#"
    {
        "dependencies": {
            "ghost_org/law": { "out-of-band-authentication": "abc123", "branch": "main" }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "dependencies.json".to_string()).unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert!(report.errors[0]
        .message
        .contains("does not exist on the filesystem"));
}

/// fonds A depends on B, B depends back on A. The recursion should detect
/// the repeat and report a helpful cycle error rather than looping forever.
#[test]
fn test_check_when_dependency_cycle_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    init_minimal_valid_fonds(archive_path.path(), "cyclic_org").unwrap();
    archive_testtools::add_dependencies(archive_path.path(), "test_org", vec!["cyclic_org"], None)
        .unwrap();

    let cyclic_auth_repo_path = archive_path.path().join("cyclic_org/law");
    let content = r#"
    {
        "dependencies": {
            "test_org/law": { "out-of-band-authentication": "abc123", "branch": "main" }
        }
    }
    "#
    .to_string();
    write_to_file(
        &cyclic_auth_repo_path,
        content,
        "dependencies.json".to_string(),
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert!(report.errors[0].message.contains("cycle"));
}

/// test_org depends on both branch_a and branch_b, and both depend on the
/// same shared_org. shared_org's own "no scopes" warning should be counted
/// once, not once per parent that reaches it.
#[test]
fn test_check_when_diamond_dependency_expect_deduplicated_warning() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();

    init_minimal_valid_fonds(archive_path.path(), "branch_a").unwrap();
    init_minimal_valid_fonds(archive_path.path(), "branch_b").unwrap();
    init_minimal_valid_fonds(archive_path.path(), "shared_org").unwrap();

    archive_testtools::add_dependencies(
        archive_path.path(),
        "test_org",
        vec!["branch_a", "branch_b"],
        None,
    )
    .unwrap();
    archive_testtools::add_dependencies(archive_path.path(), "branch_a", vec!["shared_org"], None)
        .unwrap();
    archive_testtools::add_dependencies(archive_path.path(), "branch_b", vec!["shared_org"], None)
        .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert!(
        report.errors.is_empty(),
        "unexpected errors: {:?}",
        report.errors
    );
    // branch_a's own "no scopes" warning, branch_b's own, and shared_org's
    // -- counted once even though shared_org is reached via both branches.
    assert_eq!(report.warnings.len(), 3, "warnings: {:?}", report.warnings);
    let shared_warnings = report
        .warnings
        .iter()
        .filter(|warning| warning.fonds == "shared_org/law")
        .count();
    assert_eq!(
        shared_warnings, 1,
        "shared_org should only be checked once: {:?}",
        report.warnings
    );
}

/// `repositories.json`/`dependencies.json` are read during Fonds
/// construction, so a malformed one anywhere collapses to the generic
/// archive-parse error (see above) with no fonds attribution.
/// `targets/protected/info.json` is only read by our own `check_info_json`,
/// so use that instead to prove recursion actually descends into a nested
/// Fonds and attributes the error to the right one.
#[test]
fn test_check_when_nested_dependency_invalid_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();

    init_minimal_valid_fonds(archive_path.path(), "dependent_org").unwrap();
    archive_testtools::add_dependencies(
        archive_path.path(),
        "test_org",
        vec!["dependent_org"],
        None,
    )
    .unwrap();

    let dependent_auth_repo_path = archive_path.path().join("dependent_org/law");
    write_to_file(
        &dependent_auth_repo_path,
        "{ not valid json".to_string(),
        "protected/info.json".to_string(),
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert_eq!(report.errors[0].fonds, "dependent_org/law");
    assert_eq!(report.errors[0].file, "targets/protected/info.json");
    assert!(report.errors[0].message.contains("invalid JSON"));
}

#[test]
fn test_check_when_target_file_missing_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &["law-rdf"]).unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert_eq!(report.errors[0].file, "targets/test_org/law-rdf");
    assert!(report.errors[0]
        .message
        .contains("target file does not exist"));
}

#[test]
fn test_check_when_target_file_malformed_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");

    write_to_file(
        &auth_repo_path,
        "{ not valid json".to_string(),
        "test_org/law-rdf".to_string(),
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert_eq!(report.errors[0].file, "targets/test_org/law-rdf");
    assert!(report.errors[0]
        .message
        .contains("target file can't be parsed"));
}

#[test]
fn test_check_when_target_file_missing_required_fields_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");

    let content = serde_json::to_string_pretty(&TargetsMetadata {
        branch: String::new(),
        commit: String::new(),
        build_date: Some("2024-01-01".into()),
        codified_date: Some("2024-01-01".into()),
    })
    .unwrap();
    write_to_file(&auth_repo_path, content, "test_org/law-rdf".to_string()).unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 2, "errors: {:?}", report.errors);
    assert!(report
        .errors
        .iter()
        .any(|error| error.message.contains("missing 'branch'")));
    assert!(report
        .errors
        .iter()
        .any(|error| error.message.contains("missing 'commit'")));
}

#[test]
fn test_check_when_info_json_missing_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    // Deliberately do not write targets/protected/info.json.

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert!(report
        .errors
        .iter()
        .any(|error| error.file == "targets/protected/info.json"
            && error.message.contains("required file is missing")));
}

#[test]
fn test_check_when_info_json_malformed_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");

    write_to_file(
        &auth_repo_path,
        "{ not valid json".to_string(),
        "protected/info.json".to_string(),
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert_eq!(report.errors[0].file, "targets/protected/info.json");
    assert!(report.errors[0].message.contains("invalid JSON"));
}

#[test]
fn test_check_when_scopes_missing_expect_warning() {
    // Plain basic fixture: scopes are never set, and no target files or
    // info.json are written -- but those are *errors*, not warnings, so
    // they don't interfere with asserting on warnings here.
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.warnings.len(), 1, "warnings: {:?}", report.warnings);
    assert!(report.warnings[0].message.contains("scopes"));
}

#[test]
fn test_check_when_target_file_missing_optional_dates_expect_warnings() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &["law-rdf"]).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");

    archive_testtools::add_target_file(
        &auth_repo_path,
        "test_org",
        "law-rdf",
        &TargetsMetadata {
            branch: "main".into(),
            commit: "abc123".into(),
            build_date: None,
            codified_date: None,
        },
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert!(
        report.errors.is_empty(),
        "unexpected errors: {:?}",
        report.errors
    );
    assert_eq!(report.warnings.len(), 2, "warnings: {:?}", report.warnings);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.message.contains("build-date")));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.message.contains("codified-date")));
}

#[test]
fn test_check_when_docs_repo_missing_optional_dates_expect_no_warning() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");
    archive_testtools::add_info_json(&auth_repo_path, "test_org", "law").unwrap();

    GitRepository::init(&archive_path.path().join("test_org/law-docs")).unwrap();

    // "type" is included so the (now unconditional) type-required warning
    // doesn't muddy this test, which is specifically about the date
    // exemption in check_target_file.
    let content = r#"
    {
        "scopes": ["us/example"],
        "repositories": {
            "test_org/law-docs": {
                "custom": { "type": "docs", "serve": "latest", "routes": [".*"] }
            }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "repositories.json".to_string()).unwrap();

    archive_testtools::add_target_file(
        &auth_repo_path,
        "test_org",
        "law-docs",
        &TargetsMetadata {
            branch: "main".into(),
            commit: "abc123".into(),
            build_date: None,
            codified_date: None,
        },
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert!(
        report.errors.is_empty(),
        "unexpected errors: {:?}",
        report.errors
    );
    assert!(
        report.warnings.is_empty(),
        "unexpected warnings: {:?}",
        report.warnings
    );
}

#[test]
fn test_check_when_xml_repo_missing_codified_date_expect_only_build_date_warning() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let auth_repo_path = archive_path.path().join("test_org/law");
    archive_testtools::add_info_json(&auth_repo_path, "test_org", "law").unwrap();

    let content = r#"
    {
        "scopes": ["us/example"],
        "repositories": {
            "test_org/law-xml": {
                "custom": { "type": "xml", "serve": "latest", "serve-prefix": "_xml" }
            }
        }
    }
    "#
    .to_string();
    write_to_file(&auth_repo_path, content, "repositories.json".to_string()).unwrap();

    archive_testtools::add_target_file(
        &auth_repo_path,
        "test_org",
        "law-xml",
        &TargetsMetadata {
            branch: "main".into(),
            commit: "abc123".into(),
            build_date: None,
            codified_date: None,
        },
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert!(
        report.errors.is_empty(),
        "unexpected errors: {:?}",
        report.errors
    );
    assert_eq!(report.warnings.len(), 1, "warnings: {:?}", report.warnings);
    assert!(report.warnings[0].message.contains("build-date"));
}

#[test]
fn test_check_when_dependency_route_invalid_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    init_minimal_valid_fonds(archive_path.path(), "ghost_org").unwrap();
    let content = r#"
    {
        "repositories": {
            "ghost_org/law-html": { "custom": { "serve": "latest", "routes": ["(unclosed"] } }
        }
    }
    "#
    .to_string();
    write_to_file(
        &archive_path.path().join("ghost_org/law"),
        content,
        "repositories.json".to_string(),
    )
    .unwrap();
    let dependencies = r#"
    {
        "dependencies": {
            "ghost_org/law": { "out-of-band-authentication": "abc123", "branch": "main" }
        }
    }
    "#
    .to_string();
    write_to_file(
        &archive_path.path().join("test_org/law"),
        dependencies,
        "dependencies.json".to_string(),
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert!(report.errors[0]
        .message
        .contains("invalid route '(unclosed'"));
}

#[test]
fn test_check_when_dependency_field_missing_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    make_root_valid(archive_path.path(), "test_org", &[]).unwrap();
    init_minimal_valid_fonds(archive_path.path(), "ghost_org").unwrap();
    let dependencies =
        r#"{ "dependencies": { "ghost_org/law": { "branch": "main" } } }"#.to_string();
    write_to_file(
        &archive_path.path().join("test_org/law"),
        dependencies,
        "dependencies.json".to_string(),
    )
    .unwrap();

    let report = check::check(
        archive_path.path().to_str().unwrap(),
        archive_path.path().to_path_buf(),
    )
    .unwrap();

    assert_eq!(report.errors.len(), 1, "errors: {:?}", report.errors);
    assert!(report.errors[0]
        .message
        .contains("out-of-band-authentication"));
}
