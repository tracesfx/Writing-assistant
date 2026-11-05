//! Story Data Service (SPEC §5.1).

mod ops;
mod search;
mod service;
pub mod tree;
pub mod types;

pub use ops::{COMMENTS_DIR, SCRATCHPAD_FILE};
pub use service::{APP_DIR, StoryService, revision_of};

#[cfg(test)]
mod tests;
