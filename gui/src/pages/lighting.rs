//! The Lighting page: keyboard preview on top, effect + colors + options
//! below — modeled on GNOME Settings' Appearance panel.

use aurora_protocol::effects::Effects;
use relm4::{
    adw::{self, prelude::*},
    gtk::{self, gdk},
    ComponentSender,
};
use strum::IntoEnumIterator;

use crate::{
    app::{App, AppMsg},
    preview::KeyboardPreview,
};

pub struct LightingPage {
    pub root: gtk::Widget,
    pub preview: KeyboardPreview,

    pub effect_row: adw::ComboRow,

    pub zone_buttons: [gtk::ColorDialogButton; 4],
    pub colors_group: adw::PreferencesGroup,

    pub speed_row: adw::SpinRow,
    pub brightness_row: adw::SwitchRow,
    pub direction_row: adw::ComboRow,

    pub ambient_group: adw::PreferencesGroup,
    pub fps_row: adw::SpinRow,
    pub saturation_row: adw::SpinRow,

    pub swipe_group: adw::PreferencesGroup,
    pub swipe_mode_row: adw::ComboRow,
    pub clean_row: adw::SwitchRow,
}

/// Effect names in `Effects::iter()` order; the combo row indexes into this.
pub fn effect_names() -> Vec<&'static str> {
    let mut names = Vec::new();
    for effect in Effects::iter() {
        let name: &'static str = effect.into();
        names.push(name);
    }
    names
}

pub fn build(sender: &ComponentSender<App>) -> LightingPage {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(18);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);

    // --- Preview ---------------------------------------------------------
    let preview = KeyboardPreview::new();
    content.append(&preview.root);

    // --- Effect selector -------------------------------------------------
    let effect_group = adw::PreferencesGroup::new();

    let names = effect_names();
    let effect_model = gtk::StringList::new(&names);
    let effect_row = adw::ComboRow::new();
    effect_row.set_title("Effect");
    effect_row.set_model(Some(&effect_model));

    let effect_sender = sender.clone();
    effect_row.connect_selected_notify(move |row| {
        let index = row.selected();
        if index != gtk::INVALID_LIST_POSITION {
            effect_sender.input(AppMsg::EffectSelected(index as usize));
        }
    });
    effect_group.add(&effect_row);
    content.append(&effect_group);

    // --- Zone colors ------------------------------------------------------
    let colors_group = adw::PreferencesGroup::new();
    colors_group.set_title("Zone Colors");

    let zone_row = adw::ActionRow::new();
    zone_row.set_title("Zones");
    zone_row.set_subtitle("Left to right");

    let zone_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    zone_box.set_valign(gtk::Align::Center);

    let mut zone_buttons: Vec<gtk::ColorDialogButton> = Vec::with_capacity(4);
    for zone_index in 0..4 {
        let button = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
        button.set_valign(gtk::Align::Center);

        let zone_sender = sender.clone();
        button.connect_rgba_notify(move |button| {
            let color = rgba_to_bytes(&button.rgba());
            zone_sender.input(AppMsg::ZoneColorPicked { zone_index, color });
        });

        zone_box.append(&button);
        zone_buttons.push(button);
    }
    zone_row.add_suffix(&zone_box);
    colors_group.add(&zone_row);

    // "All zones" is an action (open a picker, apply everywhere), not a
    // state display — a persistent swatch would show a stale color.
    let global_row = adw::ActionRow::new();
    global_row.set_title("All Zones");
    global_row.set_subtitle("Pick one color for the whole keyboard");
    global_row.set_activatable(true);

    let global_icon = gtk::Image::from_icon_name("color-select-symbolic");
    global_row.add_suffix(&global_icon);

    let global_sender = sender.clone();
    global_row.connect_activated(move |_| {
        global_sender.input(AppMsg::GlobalColorDialogRequested);
    });
    colors_group.add(&global_row);

    content.append(&colors_group);

    // --- Common options ---------------------------------------------------
    let options_group = adw::PreferencesGroup::new();
    options_group.set_title("Options");

    let speed_adjustment = gtk::Adjustment::new(1.0, 1.0, 10.0, 1.0, 1.0, 0.0);
    let speed_row = adw::SpinRow::new(Some(&speed_adjustment), 1.0, 0);
    speed_row.set_title("Speed");
    let speed_sender = sender.clone();
    // Signals go on the Adjustment (stable API) rather than the row.
    speed_adjustment.connect_value_changed(move |adjustment| {
        let speed = adjustment.value() as u8;
        speed_sender.input(AppMsg::SpeedPicked { speed });
    });
    options_group.add(&speed_row);

    let brightness_row = adw::SwitchRow::new();
    brightness_row.set_title("High Brightness");
    let brightness_sender = sender.clone();
    brightness_row.connect_active_notify(move |row| {
        brightness_sender.input(AppMsg::BrightnessPicked { high: row.is_active() });
    });
    options_group.add(&brightness_row);

    let direction_model = gtk::StringList::new(&["Left", "Right"]);
    let direction_row = adw::ComboRow::new();
    direction_row.set_title("Direction");
    direction_row.set_model(Some(&direction_model));
    let direction_sender = sender.clone();
    direction_row.connect_selected_notify(move |row| {
        let index = row.selected();
        if index != gtk::INVALID_LIST_POSITION {
            direction_sender.input(AppMsg::DirectionPicked { index: index as usize });
        }
    });
    options_group.add(&direction_row);

    content.append(&options_group);

    // --- Ambient-only options --------------------------------------------
    let ambient_group = adw::PreferencesGroup::new();
    ambient_group.set_title("Ambient Light");

    let fps_adjustment = gtk::Adjustment::new(30.0, 1.0, 60.0, 1.0, 5.0, 0.0);
    let fps_row = adw::SpinRow::new(Some(&fps_adjustment), 1.0, 0);
    fps_row.set_title("Frames per Second");
    let fps_sender = sender.clone();
    fps_adjustment.connect_value_changed(move |adjustment| {
        let fps = adjustment.value() as u8;
        fps_sender.input(AppMsg::AmbientFpsPicked { fps });
    });
    ambient_group.add(&fps_row);

    let saturation_adjustment = gtk::Adjustment::new(0.0, 0.0, 1.0, 0.05, 0.1, 0.0);
    let saturation_row = adw::SpinRow::new(Some(&saturation_adjustment), 0.05, 2);
    saturation_row.set_title("Saturation Boost");
    let saturation_sender = sender.clone();
    saturation_adjustment.connect_value_changed(move |adjustment| {
        let saturation = adjustment.value() as f32;
        saturation_sender.input(AppMsg::AmbientSaturationPicked { saturation });
    });
    ambient_group.add(&saturation_row);

    content.append(&ambient_group);

    // --- Swipe-only options ----------------------------------------------
    let swipe_group = adw::PreferencesGroup::new();
    swipe_group.set_title("Swipe");

    let swipe_mode_model = gtk::StringList::new(&["Change", "Fill"]);
    let swipe_mode_row = adw::ComboRow::new();
    swipe_mode_row.set_title("Mode");
    swipe_mode_row.set_model(Some(&swipe_mode_model));
    let swipe_sender = sender.clone();
    swipe_mode_row.connect_selected_notify(move |row| {
        let index = row.selected();
        if index != gtk::INVALID_LIST_POSITION {
            swipe_sender.input(AppMsg::SwipeModePicked { index: index as usize });
        }
    });
    swipe_group.add(&swipe_mode_row);

    let clean_row = adw::SwitchRow::new();
    clean_row.set_title("Clean with Black");
    clean_row.set_subtitle("Wipe to black between fills");
    let clean_sender = sender.clone();
    clean_row.connect_active_notify(move |row| {
        clean_sender.input(AppMsg::CleanWithBlackPicked { clean: row.is_active() });
    });
    swipe_group.add(&clean_row);

    content.append(&swipe_group);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(560);
    clamp.set_child(Some(&content));

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_child(Some(&clamp));
    scrolled.set_vexpand(true);

    let zone_buttons: [gtk::ColorDialogButton; 4] = match zone_buttons.try_into() {
        Ok(buttons) => buttons,
        Err(_) => unreachable!("exactly four zone buttons are created above"),
    };

    LightingPage {
        root: scrolled.upcast(),
        preview,
        effect_row,
        zone_buttons,
        colors_group,
        speed_row,
        brightness_row,
        direction_row,
        ambient_group,
        fps_row,
        saturation_row,
        swipe_group,
        swipe_mode_row,
        clean_row,
    }
}

pub fn rgba_to_bytes(rgba: &gdk::RGBA) -> [u8; 3] {
    let red = (rgba.red() * 255.0).round() as u8;
    let green = (rgba.green() * 255.0).round() as u8;
    let blue = (rgba.blue() * 255.0).round() as u8;
    [red, green, blue]
}

pub fn bytes_to_rgba(color: [u8; 3]) -> gdk::RGBA {
    gdk::RGBA::new(f32::from(color[0]) / 255.0, f32::from(color[1]) / 255.0, f32::from(color[2]) / 255.0, 1.0)
}
