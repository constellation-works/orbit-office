//! End-to-end: a Library on disk, opened through the facade, reindexed
//! and searched — the flow `office.reindex` and `office.search` will use.
//!
//! Scenarios live in sibling integration-test files, split by family:
//! `library_store_source_media`, `library_store_search_reindex`,
//! `library_store_decisions`, `library_store_writes_concurrency`,
//! `library_store_grants_symlinks`, `library_store_init_migration`,
//! `library_store_artifacts` and `library_store_proposals_sharing`.
//! Shared helpers and fixtures live in `library_store_support`.
//! This file is retained as the `library_store` test entry point; it
//! intentionally contains no scenarios of its own. Run the whole
//! `office-store` package to select every moved scenario.
#![allow(clippy::expect_used, clippy::unwrap_used)]
