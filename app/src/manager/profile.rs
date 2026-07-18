// Moved to the shared `legion-kb-protocol` crate; re-exported here to keep
// the old `crate::manager::profile::*` paths alive until `app/` is replaced.
pub use legion_kb_protocol::profile::{arr_to_zones, Profile};
