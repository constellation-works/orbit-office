//! The v2 SCR file (`SCR-<yyyymmdd>-<hhmm>-<session6>.md`: rewritten
//! sections, append-only `T<n>` Tags entries, the `previous` chain), as
//! earlier builds' scribe wrote it. This build only reads SCR files: the
//! scribe moved to the standalone scribe plugin (4_decisions.md), and the
//! session brief still reads the SCRs already in a Library as handoffs.

mod scr;

pub use scr::{
    Front, REQUESTS_GROUP, SECTION_HEADINGS, Scr, Sections, TAGS_HEADING, TagEntry, TagGroup,
    find_session, group_heading, is_v2, latest_in_scope, latest_per_scope, render, scr_id,
    scr_path, sort_groups, v2_files,
};
