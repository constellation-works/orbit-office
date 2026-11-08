//! Deterministic session continuity. All inputs are recorded data; nothing here
//! interprets a quoted entry as an instruction or performs I/O.

pub mod handoff;
pub mod listing;
pub mod scr;

mod assemble;
mod conversation;
mod entry;
mod handoff_sections;
mod helpers;
mod item;
mod scope;
mod tasks;
mod threads;

pub use assemble::{
    assemble, assemble_for_session, assemble_with_workspaces, assignment_project, handoffs,
};
pub use entry::{Entry, entries};
pub use item::{Brief, Item, Section};
pub use scope::{
    GLOBAL_HANDOFFS_LIMIT, GlobalScope, OTHER_SESSIONS_LIMIT, OTHER_SESSIONS_WINDOW_SECONDS,
    PROJECTS_LIMIT, SessionContext, looks_global, stored_scope,
};
pub use tasks::{TaskInventory, agent_runs, agent_runs_external, task_open, tasks};

pub(crate) use conversation::Conversation;
use conversation::conversations;
use helpers::{cited_assignment, cites, project_key, time_key};
pub use helpers::{invalid, markdown};
pub(crate) use threads::{closing, thread_handoffs, thread_key};
