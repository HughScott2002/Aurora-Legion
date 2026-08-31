//! The Lighting page: keyboard preview on top, effect + colors + options
//! below — modeled on GNOME Settings' Appearance panel.

use aurora_protocol::{
    effects::Effects,
    ipc::{SlotSelection, LIT_SLOTS},
    profile::SLOT_COUNT,
};
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
    /// Preview card and its caption together, so the off position can take
    /// both away rather than leaving an empty card behind.
    pub preview_box: gtk::Box,
    /// Caption under the preview naming the active slot.
    pub slot_label: gtk::Label,
    /// Shown in place of everything below the slot picker while the
    /// backlight is off.
    pub off_state: gtk::Box,

    /// Slot picker: linked toggle buttons, one per lit slot plus off.
    /// Horizontal because four short choices in a column is a list
    /// pretending to be a switch.
    pub slot_buttons: [gtk::ToggleButton; SLOT_COUNT],
    pub off_button: gtk::ToggleButton,
    /// Shown only when Fn+Space cannot be trusted, where the key would
    /// have been used.
    pub slot_note: gtk::Label,

    pub effect_row: adw::ComboRow,
    pub effect_group: adw::PreferencesGroup,

    pub zone_buttons: [gtk::ColorDialogButton; 4],
    pub colors_group: adw::PreferencesGroup,

    pub options_group: adw::PreferencesGroup,
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

/// Every slot button is the same width so the picker reads as one
/// control rather than four differently sized ones.
const SLOT_BUTTON_WIDTH_PX: i32 = 56;

/// The off mark: the page carries nothing else, so the glyph is the thing
/// that answers "what is this screen" from across a desk. Faded to stay a
/// statement of fact rather than a warning, at the opacity libadwaita's
/// own status page uses on its icon.
const OFF_ICON_PX: i32 = 160;
const OFF_ICON_OPACITY: f64 = 0.55;
/// Added to the box spacing below, so the glyph gets the wider gap
/// libadwaita puts under a status page icon and the two labels stay
/// paired.
const OFF_ICON_GAP_PX: i32 = 12;

pub fn build(sender: &ComponentSender<App>) -> LightingPage {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(18);
    content.set_margin_bottom(24);
    content.set_margin_start(12);
    content.set_margin_end(12);

    // --- Slots -----------------------------------------------------------
    // First on the page: every control below edits the selected slot, so
    // a page ordered the other way has to be read bottom-up.
    //
    // No colour chips here. The keyboard is the display, and a chip beside
    // the picker competes with the lit keys it is describing.
    let slot_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    slot_box.add_css_class("linked");
    slot_box.set_halign(gtk::Align::Center);

    // Off leads the picker: it is the position the backlight starts from
    // and returns to, and putting it first means the three lit slots read
    // left to right in their own order instead of being interrupted at the
    // end. Fn+Space still cycles in the firmware's order; this is the
    // reading order, not the cycle order.
    let off_button = gtk::ToggleButton::with_label("Off");
    off_button.set_width_request(SLOT_BUTTON_WIDTH_PX);
    off_button.set_tooltip_text(Some("Backlight off, the fourth Fn+Space position"));
    let off_sender = sender.clone();
    off_button.connect_toggled(move |button| {
        if !button.is_active() {
            return;
        }
        off_sender.input(AppMsg::SlotSelected {
            slot: SlotSelection::Off,
        });
    });
    slot_box.append(&off_button);

    let mut slot_buttons: Vec<gtk::ToggleButton> = Vec::with_capacity(SLOT_COUNT);
    for (slot_position, slot) in LIT_SLOTS.iter().enumerate() {
        let button = gtk::ToggleButton::with_label(&format!("{}", slot_position + 1));
        button.set_width_request(SLOT_BUTTON_WIDTH_PX);
        button.set_tooltip_text(Some(&format!("Slot {}", slot_position + 1)));

        let slot_sender = sender.clone();
        let selected_slot = *slot;
        button.connect_toggled(move |button| {
            if !button.is_active() {
                return; // The group deactivates the previous button too.
            }
            slot_sender.input(AppMsg::SlotSelected {
                slot: selected_slot,
            });
        });

        // One group, so the buttons behave as a picker rather than four
        // independent switches.
        button.set_group(Some(&off_button));

        slot_box.append(&button);
        slot_buttons.push(button);
    }

    let slot_note = gtk::Label::new(None);
    slot_note.add_css_class("caption");
    slot_note.add_css_class("dim-label");
    slot_note.set_wrap(true);
    slot_note.set_justify(gtk::Justification::Center);
    slot_note.set_visible(false);

    let slot_picker = gtk::Box::new(gtk::Orientation::Vertical, 6);
    slot_picker.append(&slot_box);
    slot_picker.append(&slot_note);
    content.append(&slot_picker);

    // --- Preview ---------------------------------------------------------
    // The preview and its slot caption sit in their own tighter box: the
    // page's 18px spacing is for group boundaries, the caption belongs to
    // the preview.
    let preview = KeyboardPreview::new();
    let slot_label = gtk::Label::new(None);
    slot_label.add_css_class("caption");
    slot_label.add_css_class("dim-label");
    slot_label.set_visible(false);

    let preview_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    preview_box.append(&preview.root);
    preview_box.append(&slot_label);
    content.append(&preview_box);

    // --- Effect selector -------------------------------------------------
    // Every group below the preview edits the selected slot, and the off
    // position holds no lighting to edit, so all of them are built hidden
    // and `sync_lighting_page` shows them once a lit slot is known. State
    // arrives after the window does; a group built visible would be on
    // screen during the gap and then vanish if the slot turned out to be
    // off.
    let effect_group = adw::PreferencesGroup::new();
    effect_group.set_visible(false);

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
    colors_group.set_visible(false);

    let zone_row = adw::ActionRow::new();
    zone_row.set_title("Zones");
    zone_row.set_subtitle("Left to right");

    let zone_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    zone_box.set_valign(gtk::Align::Center);

    let mut zone_buttons: Vec<gtk::ColorDialogButton> = Vec::with_capacity(4);
    for zone_index in 0..4 {
        let button = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
        button.set_valign(gtk::Align::Center);

        // Four identical unlabeled buttons are indistinguishable to a
        // screen reader (and to hover) without these.
        let description = format!("Zone {} color", zone_index + 1);
        button.set_tooltip_text(Some(&description));
        button.update_property(&[gtk::accessible::Property::Label(&description)]);

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
    options_group.set_visible(false);

    let speed_adjustment = gtk::Adjustment::new(1.0, 1.0, 10.0, 1.0, 1.0, 0.0);
    let speed_row = adw::SpinRow::new(Some(&speed_adjustment), 1.0, 0);
    speed_row.set_title("Speed");
    // Built hidden to match the default effect. `sync_lighting_page` decides
    // from then on, but it only runs once daemon state arrives, so anything
    // Static does not use would flash on screen first.
    speed_row.set_visible(false);
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
        brightness_sender.input(AppMsg::BrightnessPicked {
            high: row.is_active(),
        });
    });
    options_group.add(&brightness_row);

    let direction_model = gtk::StringList::new(&["Left", "Right"]);
    let direction_row = adw::ComboRow::new();
    direction_row.set_title("Direction");
    direction_row.set_model(Some(&direction_model));
    direction_row.set_visible(false);
    let direction_sender = sender.clone();
    direction_row.connect_selected_notify(move |row| {
        let index = row.selected();
        if index != gtk::INVALID_LIST_POSITION {
            direction_sender.input(AppMsg::DirectionPicked {
                index: index as usize,
            });
        }
    });
    options_group.add(&direction_row);

    content.append(&options_group);

    // --- Ambient-only options --------------------------------------------
    let ambient_group = adw::PreferencesGroup::new();
    ambient_group.set_title("Ambient Light");
    ambient_group.set_visible(false);

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
    swipe_group.set_visible(false);

    let swipe_mode_model = gtk::StringList::new(&["Change", "Fill"]);
    let swipe_mode_row = adw::ComboRow::new();
    swipe_mode_row.set_title("Mode");
    swipe_mode_row.set_model(Some(&swipe_mode_model));
    let swipe_sender = sender.clone();
    swipe_mode_row.connect_selected_notify(move |row| {
        let index = row.selected();
        if index != gtk::INVALID_LIST_POSITION {
            swipe_sender.input(AppMsg::SwipeModePicked {
                index: index as usize,
            });
        }
    });
    swipe_group.add(&swipe_mode_row);

    let clean_row = adw::SwitchRow::new();
    clean_row.set_title("Clean with Black");
    clean_row.set_subtitle("Wipe to black between fills");
    // Only the fill mode wipes, so the switch has nothing to mean in the
    // default change mode.
    clean_row.set_visible(false);
    let clean_sender = sender.clone();
    clean_row.connect_active_notify(move |row| {
        clean_sender.input(AppMsg::CleanWithBlackPicked {
            clean: row.is_active(),
        });
    });
    swipe_group.add(&clean_row);

    content.append(&swipe_group);

    // --- Off state --------------------------------------------------------
    // The off position has no lighting to edit and none to preview, so
    // everything above is taken away and this stands in its place. It
    // expands into whatever the slot picker leaves, which puts it in the
    // middle of the window at any size.
    //
    // Composed by hand rather than with `adw::StatusPage`, which carries
    // its own scrolled window and its own clamp; the page already sits in
    // one of each, and nesting them fights over height.
    // A slashed glyph, not a dim one. The brightness icons differ from
    // their lit versions only by weight, so at a glance they read as "on,
    // a bit", which is the one thing this position is not. The slash is
    // what makes it legible without reading the words under it.
    //
    // This name is in Adwaita, so it survives on a machine that is not
    // running the icon theme this was built against.
    let off_icon = gtk::Image::from_icon_name("night-light-disabled-symbolic");
    off_icon.set_pixel_size(OFF_ICON_PX);
    off_icon.set_opacity(OFF_ICON_OPACITY);
    off_icon.set_margin_bottom(OFF_ICON_GAP_PX);
    off_icon.set_accessible_role(gtk::AccessibleRole::Presentation);

    let off_title = gtk::Label::new(Some("Backlight off"));
    off_title.add_css_class("title-2");

    let off_body = gtk::Label::new(Some("Pick a slot to start editing lighting."));
    off_body.add_css_class("dim-label");
    off_body.set_wrap(true);
    off_body.set_justify(gtk::Justification::Center);

    let off_state = gtk::Box::new(gtk::Orientation::Vertical, 12);
    off_state.set_halign(gtk::Align::Center);
    off_state.set_valign(gtk::Align::Center);
    off_state.set_vexpand(true);
    off_state.append(&off_icon);
    off_state.append(&off_title);
    off_state.append(&off_body);
    // Built hidden: state arrives after the window does, and a mark that
    // flashes over a lit page is worse than one that waits.
    off_state.set_visible(false);
    content.append(&off_state);

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

    let slot_buttons: [gtk::ToggleButton; SLOT_COUNT] = match slot_buttons.try_into() {
        Ok(buttons) => buttons,
        Err(_) => unreachable!("one button per lit slot is created above"),
    };

    LightingPage {
        root: scrolled.upcast(),
        preview,
        preview_box,
        slot_label,
        off_state,
        slot_buttons,
        off_button,
        slot_note,
        effect_row,
        effect_group,
        zone_buttons,
        colors_group,
        options_group,
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

impl LightingPage {
    /// Move the picker to the daemon's slot. Compare before set: assigning
    /// an already-active toggle still emits `toggled`, which would send the
    /// selection straight back to the daemon.
    pub fn set_active_slot(&self, active: SlotSelection) {
        for (slot_position, button) in self.slot_buttons.iter().enumerate() {
            let should_be_active = active.index() == Some(slot_position);
            if button.is_active() != should_be_active {
                button.set_active(should_be_active);
            }
        }

        let off_should_be_active = active == SlotSelection::Off;
        if self.off_button.is_active() != off_should_be_active {
            self.off_button.set_active(off_should_be_active);
        }
    }

    /// Say why the key cannot be trusted, next to the control that
    /// replaces it. Nothing is shown when Fn+Space works, because a line
    /// saying "working" is noise.
    ///
    /// `get_visible`, not `is_visible`: the latter is false whenever an
    /// ancestor is hidden, which would skip the write and leave the label's
    /// own flag stale until the page came back into view.
    pub fn set_slot_note(&self, note: Option<&str>) {
        match note {
            Some(note) => {
                if self.slot_note.text() != note {
                    self.slot_note.set_text(note);
                }
                if !self.slot_note.get_visible() {
                    self.slot_note.set_visible(true);
                }
            }
            None => {
                if self.slot_note.get_visible() {
                    self.slot_note.set_visible(false);
                }
            }
        }
    }
}

pub fn rgba_to_bytes(rgba: &gdk::RGBA) -> [u8; 3] {
    let red = (rgba.red() * 255.0).round() as u8;
    let green = (rgba.green() * 255.0).round() as u8;
    let blue = (rgba.blue() * 255.0).round() as u8;
    [red, green, blue]
}

pub fn bytes_to_rgba(color: [u8; 3]) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(color[0]) / 255.0,
        f32::from(color[1]) / 255.0,
        f32::from(color[2]) / 255.0,
        1.0,
    )
}
