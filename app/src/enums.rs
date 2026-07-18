// Domain types moved to the shared `legion-kb-protocol` crate; these
// re-exports keep the old `crate::enums::*` paths alive until `app/` is
// replaced by the daemon + GTK GUI.
pub use legion_kb_protocol::effects::{Brightness, Direction, Effects, SwipeMode};

use crate::manager::{custom_effect::CustomEffect, profile::Profile};

#[derive(Debug)]
pub enum Message {
    CustomEffect { effect: CustomEffect },
    Profile { profile: Profile },
    Exit,
}
