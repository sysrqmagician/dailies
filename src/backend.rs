mod indexed_local_dir;
mod local_dir;

pub use indexed_local_dir::IndexedLocalDir;
pub use local_dir::{LocalDir, LocalDirBuilder};

use super::event::{Event, EventId};
use chrono::{DateTime, Local};

pub trait Backend {
  fn get_event(&self, event_id: &EventId) -> Option<Event>;

  // get events which overlap with the from..to interval.
  fn get_events(
    &self,
    from: DateTime<Local>,
    to: DateTime<Local>,
  ) -> Option<Vec<Event>>;

  fn delete_event(&mut self, event_id: &EventId) -> Option<()>;

  fn update_event(&mut self, updated_event: &Event) -> Option<()>;

  fn create_event(&mut self, event: &Event) -> Option<()>;
}
