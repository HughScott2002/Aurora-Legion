//! The Custom Effects page: saved custom effects, play/stop, load from file.

use aurora_protocol::custom_effect::CustomEffect;
use relm4::{
    adw::{self, prelude::*},
    gtk,
    ComponentSender,
};

use crate::app::{App, AppMsg};

pub struct CustomEffectsPage {
    pub root: gtk::Widget,
    group: adw::PreferencesGroup,
    pub stop_row: adw::ActionRow,
    stop_group: adw::PreferencesGroup,
    rows: Vec<adw::ActionRow>,
    empty_row: adw::ActionRow,
    shown_names: Vec<String>,
}

pub fn build(sender: &ComponentSender<App>) -> CustomEffectsPage {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(18);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);

    // Playing indicator + stop.
    let stop_group = adw::PreferencesGroup::new();
    let stop_row = adw::ActionRow::new();
    stop_row.set_title("Playing");

    let stop_button = gtk::Button::with_label("Stop");
    stop_button.add_css_class("destructive-action");
    stop_button.set_valign(gtk::Align::Center);
    let stop_sender = sender.clone();
    stop_button.connect_clicked(move |_| {
        stop_sender.input(AppMsg::CustomEffectStopped);
    });
    stop_row.add_suffix(&stop_button);
    stop_group.add(&stop_row);
    content.append(&stop_group);

    let group = adw::PreferencesGroup::new();
    group.set_title("Custom Effects");
    group.set_description(Some("Step-based effects loaded from files"));

    let open_button = gtk::Button::from_icon_name("document-open-symbolic");
    open_button.set_tooltip_text(Some("Load a custom effect file"));
    open_button.add_css_class("flat");
    let open_sender = sender.clone();
    open_button.connect_clicked(move |_| {
        open_sender.input(AppMsg::CustomEffectFileRequested);
    });
    group.set_header_suffix(Some(&open_button));

    let empty_row = adw::ActionRow::new();
    empty_row.set_title("No custom effects");
    empty_row.set_subtitle("Load one from a file with the button above");
    group.add(&empty_row);

    content.append(&group);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(560);
    clamp.set_child(Some(&content));

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_child(Some(&clamp));
    scrolled.set_vexpand(true);

    CustomEffectsPage {
        root: scrolled.upcast(),
        group,
        stop_row,
        stop_group,
        rows: Vec::new(),
        empty_row,
        shown_names: Vec::new(),
    }
}

impl CustomEffectsPage {
    pub fn sync(&mut self, effects: &[CustomEffect], playing: Option<&str>, sender: &ComponentSender<App>) {
        match playing {
            Some(name) => {
                self.stop_row.set_title(&format!("Playing “{name}”"));
                self.stop_group.set_visible(true);
            }
            None => {
                self.stop_group.set_visible(false);
            }
        }

        let mut names: Vec<String> = Vec::with_capacity(effects.len());
        for effect in effects {
            if let Some(name) = &effect.name {
                names.push(name.clone());
            }
        }

        if names == self.shown_names {
            return;
        }

        for row in self.rows.drain(..) {
            self.group.remove(&row);
        }

        self.empty_row.set_visible(names.is_empty());

        for name in &names {
            let row = adw::ActionRow::new();
            row.set_title(name);
            row.set_activatable(true);

            let play_sender = sender.clone();
            let play_name = name.clone();
            row.connect_activated(move |_| {
                play_sender.input(AppMsg::CustomEffectPlayed { name: play_name.clone() });
            });

            let delete_button = gtk::Button::from_icon_name("user-trash-symbolic");
            delete_button.add_css_class("flat");
            delete_button.set_valign(gtk::Align::Center);
            delete_button.set_tooltip_text(Some("Delete this custom effect"));
            let delete_sender = sender.clone();
            let delete_name = name.clone();
            delete_button.connect_clicked(move |_| {
                delete_sender.input(AppMsg::CustomEffectDeleted { name: delete_name.clone() });
            });
            row.add_suffix(&delete_button);

            self.group.add(&row);
            self.rows.push(row);
        }

        self.shown_names = names;
    }
}
