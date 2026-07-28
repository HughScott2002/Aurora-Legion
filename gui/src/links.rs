//! Where Aurora points people on the web, and the one way it opens a URL.
//!
//! These addresses appear in the about dialog and on the Settings page, so
//! they live here rather than being written out at each call site.

use relm4::gtk::{self, prelude::*};

pub const REPOSITORY_URL: &str = "https://github.com/HughScott2002/Aurora-Legion";
pub const NEW_ISSUE_URL: &str = "https://github.com/HughScott2002/Aurora-Legion/issues/new";
pub const DISCUSSIONS_URL: &str = "https://github.com/HughScott2002/Aurora-Legion/discussions";

/// Hand a URL to the desktop's browser.
///
/// Failure is silent on purpose. The portal already tells the user when it
/// cannot open a link, and a second complaint from Aurora would be the app
/// reporting someone else's error message twice.
pub fn open(url: &str) {
    let Some(window) = relm4::main_application().active_window() else {
        return;
    };

    let launcher = gtk::UriLauncher::new(url);
    launcher.launch(Some(&window), gtk::gio::Cancellable::NONE, |_result| {});
}
