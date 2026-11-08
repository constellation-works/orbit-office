//! Knowledge pages (About and How-to): the frontmatter model, the base
//! rules `office.knowledge-write` enforces, citation and anchor resolution,
//! template checks and the model view (specs/library.md#knowledge-pages,
//! specs/knowledge-templates.md, specs/library-bootstrap.md#page-rules).

mod citation;
mod contacts;
mod parse;
mod rules;
mod types;
mod view;

pub(crate) use citation::inline_citations;
pub use citation::{PageFile, page_kind_for_id, parse_citation, resolve_anchor};
pub use contacts::withhold_contacts;
pub(super) use parse::is_slug;
pub use types::{
    Citation, MAX_PAGE_WORDS, POLICY_TAG, Page, PageRef, PageStatus, Performable, RefTarget,
    SCRIBE_RULES_PAGE, Submission, SubmissionVia,
};
pub use view::{PageView, RefView};
