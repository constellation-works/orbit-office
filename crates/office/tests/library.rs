//! Through the built binary: `office.init` scaffolds a Library idempotently,
//! `office.reindex` rebuilds a deleted index, `office.status` reports
//! Library health, and `office.knowledge-write`/`office.knowledge-show`
//! validate, commit and read pages (STD-04: temp roots, no real host state).
//!
//! Scenario modules below share `library_support`; the sibling capture,
//! read, video and sharing suites remain part of this one integration target.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Integration scenarios fail immediately when fixture setup or assertions fail"
)]

mod library_support;

use library_support::{BIN, Env, git};

#[path = "library/init_config.rs"]
mod init_config;

#[path = "library/status.rs"]
mod status;

#[path = "library/capture.rs"]
mod capture;

#[path = "library/knowledge.rs"]
mod knowledge;

#[path = "library/context.rs"]
mod context;

#[path = "library/decisions.rs"]
mod decisions;

#[path = "library/bootstrap.rs"]
mod bootstrap;

#[path = "document_capture/mod.rs"]
mod document_capture;

#[path = "library_reads/mod.rs"]
mod library_reads;

#[cfg(unix)]
#[path = "video_pipeline/mod.rs"]
mod video_pipeline;

#[path = "shared_library/mod.rs"]
mod shared_library;
