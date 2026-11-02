//! Scribe v2 (specs/scribe.md): model-free marker detection and the trust
//! rule, the v2 SCR file (`SCR-<yyyymmdd>-<hhmm>-<session6>.md`, rewritten
//! sections, append-only `T<n>` Tags entries, the `previous` chain), and
//! the pure pass planner `office.scribe-append` writes from. v1 files stay
//! readable through the v1 readers beside this module.

pub mod markers;
mod pass;
mod scr;

pub use markers::{Author, Marker, TURN_MAX_CHARS, TURN_MAX_LINES, detect, session_scope};
pub use pass::{
    PREFERENCE_DIR, PassPlan, PassRequest, PassStores, PlannedFile, TITLE_WORDS, plan_pass,
    preference_page_id, preference_page_path,
};
pub use scr::{
    Front, REQUESTS_GROUP, SECTION_HEADINGS, Scr, Sections, TAGS_HEADING, TagEntry, TagGroup,
    find_session, group_heading, is_v2, latest_in_scope, latest_per_scope, render, scr_id,
    scr_path, sort_groups, v2_files,
};
