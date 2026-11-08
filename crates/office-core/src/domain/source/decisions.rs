//! Decision ledgers (specs/decisions.md, "Decision ledger"): one append-only
//! file per scope at `sources/decisions/<scope>.md`, id `decisions-<scope>`,
//! one `dec-<scope>-<n>` entry per body line in the D13 schema.
//!
//! Pure: parsing, rendering, validation, `reversed_by` derivation, id and
//! alias resolution. The Library adapter reads and writes the files.

mod decision;
mod input;
mod ledger;
mod matching;
mod note;
mod scope;
mod types;
mod views;

pub use decision::Decision;
pub use input::decision_from_input;
pub use ledger::{EvidenceRef, Ledger, LedgerEntry, PlannedFile, append_line};
pub use matching::{chosen_matches, parse_entry_id, same_as_match};
pub use note::{NoteSource, note_entry, note_scope, note_source};
pub use scope::{
    KIND, LEDGER_DIR, Scope, decision_input_required, is_ledger_path, source_kind_required,
};
pub use types::{Cited, Context, DecidedBy, Options, OrbitInfo, OrbitModel};
pub use views::{EntryView, Resolved, entry_record, is_entry_record, resolve, views};
