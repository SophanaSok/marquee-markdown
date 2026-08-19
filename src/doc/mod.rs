//! Document state: the layout cache and the reading position.
//!
//! Kept separate from the application shell so the reader's model of a
//! document can be exercised without a terminal.

pub mod cache;
pub mod outline;
pub mod search;
pub mod view;

pub use cache::DocCache;
pub use outline::Outline;
pub use search::Search;
pub use view::{Extent, View};
