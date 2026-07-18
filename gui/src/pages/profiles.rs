//! The Profiles page: saved profiles as a boxed list, activate to apply,
//! plus save-current-as and delete.

use legion_kb_protocol::profile::Profile;
use relm4::{
    adw::{self, prelude::*},
    gtk,
    ComponentSender,
};

use crate::app::{App, AppMsg};

pub struct ProfilesPage {
    pub root: gtk::Widget,
    group: adw::PreferencesGroup,
    /// Rows currently shown; kept so a rebuild can remove them explicitly.
    rows: Vec<adw::ActionRow>,
    empty_row: adw::ActionRow,
    shown_names: Vec<String>,
}

pub fn build(sender: &ComponentSender<App>) -> ProfilesPage {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(18);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let group = adw::PreferencesGroup::new();
    group.set_title("Saved Profiles");
    group.set_description(Some("Activate a profile to apply it. Meta+Right Alt cycles through them."));

    let save_button = gtk::Button::from_icon_name("document-new-symbolic");
    save_button.set_tooltip_text(Some("Save current settings as a profile"));
    save_button.add_css_class("flat");
    let save_sender = sender.clone();
    save_button.connect_clicked(move |_| {
        save_sender.input(AppMsg::SaveProfileDialogRequested);
    });
    group.set_header_suffix(Some(&save_button));

    let empty_row = adw::ActionRow::new();
    empty_row.set_title("No saved profiles");
    empty_row.set_subtitle("Save the current settings with the + button above");
    group.add(&empty_row);

    content.append(&group);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(560);
    clamp.set_child(Some(&content));

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_child(Some(&clamp));
    scrolled.set_vexpand(true);

    ProfilesPage {
        root: scrolled.upcast(),
        group,
        rows: Vec::new(),
        empty_row,
        shown_names: Vec::new(),
    }
}

impl ProfilesPage {
    /// Rebuild the list when the set of saved profiles changed.
    pub fn sync(&mut self, profiles: &[Profile], current_name: Option<&str>, sender: &ComponentSender<App>) {
        let mut names: Vec<String> = Vec::with_capacity(profiles.len());
        for profile in profiles {
            if let Some(name) = &profile.name {
                names.push(name.clone());
            }
        }

        if names == self.shown_names {
            self.update_current_marker(current_name);
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

            let apply_sender = sender.clone();
            let apply_name = name.clone();
            row.connect_activated(move |_| {
                apply_sender.input(AppMsg::ProfileActivated { name: apply_name.clone() });
            });

            let delete_button = gtk::Button::from_icon_name("user-trash-symbolic");
            delete_button.add_css_class("flat");
            delete_button.set_valign(gtk::Align::Center);
            delete_button.set_tooltip_text(Some("Delete this profile"));
            let delete_sender = sender.clone();
            let delete_name = name.clone();
            delete_button.connect_clicked(move |_| {
                delete_sender.input(AppMsg::ProfileDeleted { name: delete_name.clone() });
            });
            row.add_suffix(&delete_button);

            self.group.add(&row);
            self.rows.push(row);
        }

        self.shown_names = names;
        self.update_current_marker(current_name);
    }

    /// Show which saved profile is live via the row subtitle.
    fn update_current_marker(&self, current_name: Option<&str>) {
        for (row, name) in self.rows.iter().zip(self.shown_names.iter()) {
            if Some(name.as_str()) == current_name {
                row.set_subtitle("Current");
            } else {
                row.set_subtitle("");
            }
        }
    }
}
