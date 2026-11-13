-- Add up migration script here
CREATE INDEX IF NOT EXISTS document_change_doc_mpath_fk_idx ON document_change(doc_mpath);
CREATE INDEX IF NOT EXISTS publication_version_publication_id_idx ON publication_version(publication_id);
CREATE INDEX IF NOT EXISTS publication_last_valid_publication_id_idx ON publication(last_valid_publication_id);
CREATE INDEX IF NOT EXISTS data_repo_commits_publication_id_idx ON data_repo_commits(publication_id);

PRAGMA optimize;
