# Changelog

All _notable_ changes to this project will be documented in this file.

The format is based on _[Keep a Changelog][keepachangelog]_,
and this project adheres to a _modified_ form of _[Semantic Versioning][semver]_
(major version is the year; minor and patch are the same).

## [Unreleased]

### Added

- Add `POST /_internal/redirects/refresh` so a running server can pick up redirects inserted by `taf-server update` without restarting  ([#132])

### Changed

### Fixed

### Removed

[#132]: https://github.com/openlawlibrary/taf-server/pull/132

## [v0.7.1]

### Added

### Changed

### Fixed

- Release the `_date` handler's database transaction before acquiring further pool connections, fixing a connection pool deadlock under concurrent requests ([#134])

### Removed

## [v0.7.0]

### Added

### Changed

### Fixed

- Build the Windows release binary by installing libxml2 through vcpkg in CI ([#130])

### Removed

[#134]: https://github.com/openlawlibrary/taf-server/pull/134
[#130]: https://github.com/openlawlibrary/taf-server/pull/130

## [v0.7.0-alpha.1]

### Added

- Add command for validating the root archive: `taf-server check` ([#122])

### Changed

-  Rename project to taf-server ([#123])
-  Upgrade Rust to 1.97.1 ([#115])
-  Update `Cargo.toml` dependencies ([#115])
-  Store repository redirects in the database and resolve them at request time instead of registering static routes at startup ([#111])

### Fixed

- Stop labelling versions of a historical (non-current) publication as "Current" in the `/_api/versions` response. ([#128])
- Rebuild a fonds whose non-revoked publications are missing `html_data_repo_name`. ([#127])
- Fixed non-deterministic commit selection on tied codified/build dates in find_commit_by_pub_id_and_version_date ([#116])

### Removed

- Re-ingest changes for a codified date when it is built again ([#124])

[#128]: https://github.com/openlawlibrary/taf-server/pull/128
[#127]: https://github.com/openlawlibrary/taf-server/pull/127
[#124]: https://github.com/openlawlibrary/taf-server/pull/124
[#123]: https://github.com/openlawlibrary/taf-server/pull/123
[#122]: https://github.com/openlawlibrary/taf-server/pull/122
[#116]: https://github.com/openlawlibrary/taf-server/pull/116
[#115]: https://github.com/openlawlibrary/taf-server/pull/115
[#111]: https://github.com/openlawlibrary/taf-server/pull/111

## [v0.6.9]

### Added

### Changed

### Fixed

- Resolve archived HTML repo for historical publications in `_date` endpoint

### Removed

[#116]: https://github.com/openlawlibrary/taf-server/pull/116
[#115]: https://github.com/openlawlibrary/taf-server/pull/115

## [v0.6.8]

### Added

- Added support for ETags ([#98])
- Add redirects for dynamic routes ([#97])
- Added _date endpoint from portal into taf-server ([#100]) 
- Added _publication endpoint for historical version ([#104])

### Changed

### Fixed

### Removed

[#104]: https://github.com/openlawlibrary/taf-server/pull/104
[#100]: https://github.com/openlawlibrary/taf-server/pull/100
[#98]: https://github.com/openlawlibrary/taf-server/pull/98
[#97] https://github.com/openlawlibrary/taf-server/pull/97


## [v0.6.7]

### Added

- Add --force flag and automatic inconsistency detection to taf-server update ([#108])

### Changed

### Fixed

### Removed

[#108]: https://github.com/openlawlibrary/taf-server/pull/108

## [v0.6.6]

### Fixed

- Fix commit insertion for multiple data repos of the same type ([#107])

[#107]: https://github.com/openlawlibrary/taf-server/pull/107

## [v0.6.5]

### Added

- Track html data repo per publication for multi-repo support ([#106]) 

### Changed

### Fixed

### Removed

[#106]: https://github.com/openlawlibrary/taf-server/pull/106

## [v0.6.4]

### Added

### Changed

### Fixed

- Fix database connection parameters
- Fix `current` version and publication name case sensitivity

### Removed

[#102]: https://github.com/openlawlibrary/taf-server/pull/102


## [v0.6.3]

### Added

### Changed

### Fixed

- Fix(history, changes) fix publication search ([#95])

[#95]: https://github.com/openlawlibrary/taf-server/pull/95

### Removed

## [v0.6.2]

### Added

### Changed

### Fixed

- Fix(versions): insert into current when publication param is missing ([#93])

[#93]: https://github.com/openlawlibrary/taf-server/pull/93

### Removed

## [v0.6.1]

### Added

### Changed

### Fixed

- Resolve `_api/versions` inconsistencies ([#92])

### Removed

[#92]: https://github.com/openlawlibrary/taf-server/pull/92

## [v0.6.0]

### Added

- Added bind_to argument in serve command ([#78])
- Make CLI more generic, any Rust project that depends on taf-server can now re-export CLI ([#89])
- Add `from_path` associated function to init `Repo` ([#89])
- Add `_archive` endpoint ([#81])
- Restrict `_archive` endpoint access on private repositories ([#81])

### Changed

- Bump rust-version to `1.88` ([#86])

### Fixed

### Removed

[#89]: https://github.com/openlawlibrary/taf-server/pull/89
[#86]: https://github.com/openlawlibrary/taf-server/pull/86
[#81]: https://github.com/openlawlibrary/taf-server/pull/81
[#78]: https://github.com/openlawlibrary/taf-server/pull/78

## [v0.5.1]

### Added

- Add `--include/-i` and `--exclude/-e` to `taf-server update` ([#88])

### Changed

### Fixed

- Fix(history, changes): skip iterating over publication when revoked ([#88])

### Removed

[#88]: https://github.com/openlawlibrary/taf-server/pull/88

## [v0.5.0]

### Added

- Add `build_date` and `codified_date` columns to `data_repo_commits` table ([#76])
- Add `X-File-Path` header to `_archive` and git microserver HTTP responses ([#70])
- Added tests fot `taf-server git` ([64])

### Changed

- Merged `taf-server git` and `taf-server serve` into single command ([#64])

### Fixed

- git serve now support committish that contains / in name ([#64])

### Removed

[#76]: https://github.com/openlawlibrary/taf-server/pull/76
[#70]: https://github.com/openlawlibrary/taf-server/pull/70
[#64]: https://github.com/openlawlibrary/taf-server/pull/64

## [v0.4.2]

### Added

### Changed

### Fixed

- Turn error into a warning when fonds rdf repo is not setup ([#75])
- Fix insert hashes on non-revoked publications ([#73])

### Removed

[#75]: https://github.com/openlawlibrary/taf-server/pull/75
[#73]: https://github.com/openlawlibrary/taf-server/pull/73


## [v0.4.1]

### Added

### Changed

### Fixed

- Add missing route to versions endpoint ([#68])

### Removed

[#68]: https://github.com/openlawlibrary/taf-server/pull/68

## [v0.4.0]

### Added

- Insert commit hashes in the database ([63])

### Changed

- Allow HEAD requests for dynamic routes ([58])
- Rename metadata directory to `.taf` dir ([61])
- Bump rust-version to `1.83` ([61])

### Fixed

### Removed

[#63]: https://github.com/openlawlibrary/taf-server/pull/63
[#61]: https://github.com/openlawlibrary/taf-server/pull/61
[#58]: https://github.com/openlawlibrary/taf-server/pull/58

## [v0.3.2]

### Added

### Changed

### Fixed

- Fix `taf-server update` partial update when there are new publications ([#56])

### Removed

[#56]: https://github.com/openlawlibrary/taf-server/pull/56

## [v0.3.1]

### Added

### Changed

### Fixed

- Fix resolve to `_api/versions` requests without trailing `/` ([#52])

### Removed

[#52]: https://github.com/openlawlibrary/taf-server/pull/52

## [v0.3.0]

### Added

- Add filesystem logging ([#42])
- Add command to insert history into database from RDF ([#33], [#42], [#44], [#46])
- Add versions endpoint to view dates on which documents and/or collections have changed ([#33])
- Add command to serve current documents from repositories ([#32])

### Changed

### Fixed

- Load paths to git repositories at start-time ([#47])
- Fixes to insert history command ([#46])

### Removed

[#47]: https://github.com/openlawlibrary/taf-server/pull/47
[#46]: https://github.com/openlawlibrary/taf-server/pull/46
[#44]: https://github.com/openlawlibrary/taf-server/pull/44
[#42]: https://github.com/openlawlibrary/taf-server/pull/42
[#33]: https://github.com/openlawlibrary/taf-server/pull/33
[#32]: https://github.com/openlawlibrary/taf-server/pull/32

## [v0.2.1]

### Added

- Add basic instruments to git serve ([#26])

### Changed

### Fixed

- Fix safe directory issues by upgrading git2 to latest version ([#26])

### Removed

[#26]: https://github.com/openlawlibrary/taf-server/pull/26

## [v0.2.0]

### Added

### Changed

- Update license, release under AGPL ([#25])

### Fixed

### Removed

[#25]: https://github.com/openlawlibrary/taf-server/pull/25

## [v0.1.1]

### Added

### Changed

### Fixed

- default to `text/html` mime-type instead of `application/octet-stream`

### Removed

## [v0.1.0]

### Added

- Added `/{namespace}/{name}/{commitish}{remainder}` endpoint (initial commits)
- Added ci/cd and local verbose clippy in vscode

### Changed

### Fixed

### Removed

[Unreleased]: https://github.com/openlawlibrary/taf-server/compare/v0.7.1...HEAD
[v0.7.1]: https://github.com/openlawlibrary/taf-server/compare/v0.7.0...v0.7.1
[v0.7.0]: https://github.com/openlawlibrary/taf-server/compare/v0.7.0-alpha.1...v0.7.0
[v0.7.0-alpha.1]: https://github.com/openlawlibrary/taf-server/compare/v0.6.9...v0.7.0-alpha.1
[v0.6.9]: https://github.com/openlawlibrary/taf-server/compare/v0.6.8...v0.6.9
[v0.6.8]: https://github.com/openlawlibrary/taf-server/compare/v0.6.7...v0.6.8
[v0.6.7]: https://github.com/openlawlibrary/taf-server/compare/v0.6.6...v0.6.7
[v0.6.6]: https://github.com/openlawlibrary/taf-server/compare/v0.6.5...v0.6.6
[v0.6.5]: https://github.com/openlawlibrary/taf-server/compare/v0.6.4...v0.6.5
[v0.6.4]: https://github.com/openlawlibrary/taf-server/compare/v0.6.3...v0.6.4
[v0.6.3]: https://github.com/openlawlibrary/taf-server/compare/v0.6.2...v0.6.3
[v0.6.2]: https://github.com/openlawlibrary/taf-server/compare/v0.6.1...v0.6.2
[v0.6.1]: https://github.com/openlawlibrary/taf-server/compare/v0.6.0...v0.6.1
[v0.6.0]: https://github.com/openlawlibrary/taf-server/compare/v0.5.1...v0.6.0
[v0.5.1]: https://github.com/openlawlibrary/taf-server/compare/v0.5.0...v0.5.1
[v0.5.0]: https://github.com/openlawlibrary/taf-server/compare/v0.4.2...v0.5.0
[v0.4.2]: https://github.com/openlawlibrary/taf-server/compare/v0.4.1...v0.4.2
[v0.4.1]: https://github.com/openlawlibrary/taf-server/compare/v0.4.0...v0.4.1
[v0.4.0]: https://github.com/openlawlibrary/taf-server/compare/v0.3.2...v0.4.0
[v0.3.2]: https://github.com/openlawlibrary/taf-server/compare/v0.3.1...v0.3.2
[v0.3.1]: https://github.com/openlawlibrary/taf-server/compare/v0.3.0...v0.3.1
[v0.3.0]: https://github.com/openlawlibrary/taf-server/compare/v0.2.1...v0.3.0
[v0.2.1]: https://github.com/openlawlibrary/taf-server/compare/v0.2.0...v0.2.1
[v0.2.0]: https://github.com/openlawlibrary/taf-server/compare/v0.1.1...v0.2.0
[v0.1.1]: https://github.com/openlawlibrary/taf-server/compare/v0.1.0...v0.1.1
[v0.1.0]: https://github.com/openlawlibrary/taf-server/compare/2b01423c06369f5f0f168ae4c4698371d713ede7...v0.1.0

[keepachangelog]: https://keepachangelog.com/en/1.0.0/
[semver]: https://semver.org/spec/v2.0.0.html