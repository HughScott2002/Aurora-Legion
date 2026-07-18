// Moved to the shared `legion-kb-protocol` crate; re-exported here to keep
// the old `crate::manager::custom_effect::*` paths alive until `app/` is replaced.
pub use legion_kb_protocol::custom_effect::{CustomEffect, EffectType};
