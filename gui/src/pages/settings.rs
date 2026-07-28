//! The Settings page: the background service, and where to reach the
//! project.
//!
//! Named for what a user came here to do, not for the process it happens to
//! configure. "Daemon" is a word this app taught them, and teaching a word
//! is a cost the page has to earn.

use relm4::{
    adw::{self, prelude::*},
    gtk,
    ComponentSender,
};

use crate::{
    app::{App, AppMsg},
    links,
};

pub struct SettingsPage {
    pub root: gtk::Widget,
    pub status_row: adw::ActionRow,
    pub autostart_row: adw::SwitchRow,
}

pub fn build(sender: &ComponentSender<App>) -> SettingsPage {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(18);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);

    // --- Background service ----------------------------------------------
    let service_group = adw::PreferencesGroup::new();
    service_group.set_title("Background Service");
    service_group.set_description(Some("Keeps effects running after the window closes"));

    let status_row = adw::ActionRow::new();
    status_row.set_title("Status");
    status_row.set_subtitle("Checking\u{2026}");
    service_group.add(&status_row);

    let restart_row = adw::ActionRow::new();
    restart_row.set_title("Restart");
    restart_row.set_subtitle("Stop and start it again");
    let restart_button = gtk::Button::from_icon_name("view-refresh-symbolic");
    restart_button.add_css_class("flat");
    restart_button.set_valign(gtk::Align::Center);
    restart_button.set_tooltip_text(Some("Restart the background service"));
    let restart_sender = sender.clone();
    restart_button.connect_clicked(move |_| {
        restart_sender.input(AppMsg::DaemonRestartRequested);
    });
    restart_row.add_suffix(&restart_button);
    restart_row.set_activatable_widget(Some(&restart_button));
    service_group.add(&restart_row);

    let autostart_row = adw::SwitchRow::new();
    autostart_row.set_title("Start at Login");
    autostart_row.set_subtitle("Enable the systemd user service");
    let autostart_sender = sender.clone();
    autostart_row.connect_active_notify(move |row| {
        autostart_sender.input(AppMsg::AutostartToggled { enabled: row.is_active() });
    });
    service_group.add(&autostart_row);

    content.append(&service_group);

    // --- Project ----------------------------------------------------------
    // Aurora is tested on one laptop. The report link is the only way a
    // failure on any other model reaches the person who can fix it, so it
    // belongs on a page rather than buried in a menu.
    let project_group = adw::PreferencesGroup::new();
    project_group.set_title("Project");

    let issue_row = link_row("Report an Issue", "Bugs, or a laptop Aurora does not drive properly", links::NEW_ISSUE_URL);
    project_group.add(&issue_row);

    let discussions_row = link_row("Ask a Question", "Discussions on GitHub", links::DISCUSSIONS_URL);
    project_group.add(&discussions_row);

    let star_row = link_row("Star on GitHub", "Helps other Legion owners find Aurora", links::REPOSITORY_URL);
    project_group.add(&star_row);

    content.append(&project_group);

    // --- Credit -----------------------------------------------------------
    // A footer, not a group: this is the smallest thing on the page and
    // should read that way.
    let credit = gtk::Label::new(Some(&format!("Aurora {} \u{00b7} GPL-3.0 \u{00b7} Hugh Scott", env!("CARGO_PKG_VERSION"))));
    credit.add_css_class("caption");
    credit.add_css_class("dim-label");
    credit.set_wrap(true);
    credit.set_justify(gtk::Justification::Center);
    content.append(&credit);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(560);
    clamp.set_child(Some(&content));

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_child(Some(&clamp));
    scrolled.set_vexpand(true);

    SettingsPage {
        root: scrolled.upcast(),
        status_row,
        autostart_row,
    }
}

/// A row that leaves the app. The arrow icon is libadwaita's own marker for
/// that, so the row says where it goes before it is pressed.
fn link_row(title: &str, subtitle: &str, url: &'static str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_activatable(true);

    let icon = gtk::Image::from_icon_name("adw-external-link-symbolic");
    row.add_suffix(&icon);

    row.connect_activated(move |_| {
        links::open(url);
    });

    row
}
