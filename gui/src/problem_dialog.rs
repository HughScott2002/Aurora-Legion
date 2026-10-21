//! Optional help and diagnostics, kept out of the main lighting controls.

use relm4::{
    adw::{self, prelude::*},
    gtk,
};

use crate::{
    links,
    problems::{Help, Problem},
};

const MAX_DETAILS_BYTES: usize = 8192;
const DETAILS_HEIGHT: i32 = 180;

pub fn show(problem: &Problem) {
    let Some(window) = relm4::main_application().active_window() else {
        return;
    };
    let mut details = format!(
        "{}\nAurora {}\nProtocol {}\n\n{}",
        problem.title,
        env!("CARGO_PKG_VERSION"),
        aurora_protocol::ipc::PROTOCOL_VERSION,
        problem.details,
    );
    if details.len() > MAX_DETAILS_BYTES {
        let mut end = MAX_DETAILS_BYTES;
        while !details.is_char_boundary(end) {
            end -= 1;
        }
        details.truncate(end);
        details.push_str("\nFurther details were omitted.");
    }
    let label = gtk::Label::new(Some(&details));
    label.set_selectable(true);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    label.add_css_class("monospace");
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(DETAILS_HEIGHT)
        .propagate_natural_height(true)
        .child(&label)
        .build();
    let expander = gtk::Expander::new(Some("Technical Details"));
    expander.set_child(Some(&scroll));
    let dialog = adw::AlertDialog::new(Some(problem.title), Some(problem.description));
    dialog.set_extra_child(Some(&expander));
    dialog.add_response("close", "Close");
    dialog.add_response("copy", "Copy Details");
    let guide = match problem.help {
        Help::Setup | Help::Update => Some(links::SETUP_GUIDE_URL),
        Help::Connection | Help::Recovering => Some(links::CONNECTION_HELP_URL),
        _ => None,
    };
    if guide.is_some() {
        dialog.add_response("guide", "Open Guide");
        dialog.set_response_appearance("guide", adw::ResponseAppearance::Suggested);
    }
    dialog.set_close_response("close");
    dialog.connect_response(None, move |_, response| {
        if response == "copy" {
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&details);
            }
        }
        if response == "guide" {
            if let Some(guide) = guide {
                links::open(guide);
            }
        }
    });
    dialog.present(Some(&window));
}
