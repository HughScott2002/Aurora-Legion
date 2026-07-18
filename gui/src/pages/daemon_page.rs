//! The Daemon page: status, restart, start-at-login.

use relm4::{
    adw::{self, prelude::*},
    gtk,
    ComponentSender,
};

use crate::app::{App, AppMsg};

pub struct DaemonPage {
    pub root: gtk::Widget,
    pub status_row: adw::ActionRow,
    pub autostart_row: adw::SwitchRow,
}

pub fn build(sender: &ComponentSender<App>) -> DaemonPage {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(18);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let group = adw::PreferencesGroup::new();
    group.set_title("Daemon");
    group.set_description(Some("The background service that drives the keyboard"));

    let status_row = adw::ActionRow::new();
    status_row.set_title("Status");
    status_row.set_subtitle("Checking…");
    group.add(&status_row);

    let restart_row = adw::ActionRow::new();
    restart_row.set_title("Restart");
    restart_row.set_subtitle("Stop and start the daemon");
    let restart_button = gtk::Button::from_icon_name("view-refresh-symbolic");
    restart_button.add_css_class("flat");
    restart_button.set_valign(gtk::Align::Center);
    let restart_sender = sender.clone();
    restart_button.connect_clicked(move |_| {
        restart_sender.input(AppMsg::DaemonRestartRequested);
    });
    restart_row.add_suffix(&restart_button);
    restart_row.set_activatable_widget(Some(&restart_button));
    group.add(&restart_row);

    let autostart_row = adw::SwitchRow::new();
    autostart_row.set_title("Start at Login");
    autostart_row.set_subtitle("Enable the systemd user service");
    let autostart_sender = sender.clone();
    autostart_row.connect_active_notify(move |row| {
        autostart_sender.input(AppMsg::AutostartToggled { enabled: row.is_active() });
    });
    group.add(&autostart_row);

    content.append(&group);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(560);
    clamp.set_child(Some(&content));

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_child(Some(&clamp));
    scrolled.set_vexpand(true);

    DaemonPage {
        root: scrolled.upcast(),
        status_row,
        autostart_row,
    }
}
