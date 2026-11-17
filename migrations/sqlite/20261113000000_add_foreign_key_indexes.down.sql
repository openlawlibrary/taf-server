-- Add down migration script here
DROP INDEX IF EXISTS document_change_doc_mpath_fk_idx;
DROP INDEX IF EXISTS publication_version_publication_id_idx;
DROP INDEX IF EXISTS publication_last_valid_publication_id_idx;
DROP INDEX IF EXISTS data_repo_commits_publication_id_idx;
