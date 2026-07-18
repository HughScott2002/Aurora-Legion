//! Root component. Written as a manual `SimpleComponent` (no view! macro)
//! so every widget update and every message is visible as plain Rust:
//! signals send `AppMsg`, `update` mutates the model and talks to the
//! daemon, `update_view` syncs widgets from the model with explicit
//! compares (which is also what stops signal echo loops).

use std::cell::Cell;

use aurora_protocol::{
    effects::{Brightness, Direction, Effects, SwipeMode},
    ipc::{DaemonState, KeyboardStatus, Request},
    profile::Profile,
};
use relm4::{
    adw::{self, prelude::*},
    gtk, ComponentParts, ComponentSender, SimpleComponent,
};
use strum::IntoEnumIterator;

use crate::{
    daemon_actions,
    ipc::{self, IpcHandle, IpcUpdate},
    pages::{custom_effects, daemon_page, lighting, profiles},
};

const WINDOW_DEFAULT_WIDTH: i32 = 640;
const WINDOW_DEFAULT_HEIGHT: i32 = 720;

pub struct App {
    connected: bool,
    state: Option<DaemonState>,
    /// Optimistic copy of the live profile; widget edits land here first.
    profile: Profile,
    ipc: IpcHandle,

    autostart_available: bool,
    autostart_enabled: bool,
    autostart_managed: bool,

    /// Toast queued by update(), shown and cleared by update_view().
    pending_toast: Cell<Option<String>>,
}

#[derive(Debug)]
pub enum AppMsg {
    Ipc(IpcUpdate),

    EffectSelected(usize),
    ZoneColorPicked { zone_index: usize, color: [u8; 3] },
    GlobalColorDialogRequested,
    GlobalColorPicked { color: [u8; 3] },
    SpeedPicked { speed: u8 },
    BrightnessPicked { high: bool },
    DirectionPicked { index: usize },
    SwipeModePicked { index: usize },
    CleanWithBlackPicked { clean: bool },
    AmbientFpsPicked { fps: u8 },
    AmbientSaturationPicked { saturation: f32 },

    ProfileActivated { name: String },
    ProfileDeleted { name: String },
    SaveProfileDialogRequested,
    SaveProfileConfirmed { name: String },

    CustomEffectPlayed { name: String },
    CustomEffectDeleted { name: String },
    CustomEffectStopped,
    CustomEffectFileRequested,
    CustomEffectFileChosen { path: std::path::PathBuf },

    StartDaemonRequested,
    DaemonRestartRequested,
    AutostartToggled { enabled: bool },
    AutostartQueried { available: bool, enabled: bool, managed: bool },
    ServiceActionFinished { description: String, error: Option<String> },
}

pub struct AppWidgets {
    toast_overlay: adw::ToastOverlay,
    permission_banner: adw::Banner,
    content_stack: gtk::Stack,

    lighting: lighting::LightingPage,
    profiles: profiles::ProfilesPage,
    custom: custom_effects::CustomEffectsPage,
    daemon: daemon_page::DaemonPage,
}

impl SimpleComponent for App {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type Root = adw::ApplicationWindow;
    type Widgets = AppWidgets;

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .title("Aurora")
            .default_width(WINDOW_DEFAULT_WIDTH)
            .default_height(WINDOW_DEFAULT_HEIGHT)
            .build()
    }

    fn init(_init: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // --- Connection worker -------------------------------------------
        let ipc_sender = sender.clone();
        let ipc = ipc::spawn(move |update| {
            ipc_sender.input(AppMsg::Ipc(update));
        });

        let model = App {
            connected: false,
            state: None,
            profile: Profile::default(),
            ipc,
            autostart_available: false,
            autostart_enabled: false,
            autostart_managed: false,
            pending_toast: Cell::new(None),
        };

        // --- Pages --------------------------------------------------------
        let lighting = lighting::build(&sender);
        let profiles = profiles::build(&sender);
        let custom = custom_effects::build(&sender);
        let daemon = daemon_page::build(&sender);

        let view_stack = adw::ViewStack::new();
        let lighting_page = view_stack.add_titled(&lighting.root, Some("lighting"), "Lighting");
        lighting_page.set_icon_name(Some("keyboard-brightness-symbolic"));
        let profiles_page = view_stack.add_titled(&profiles.root, Some("profiles"), "Profiles");
        profiles_page.set_icon_name(Some("view-list-bullet-symbolic"));
        let custom_page = view_stack.add_titled(&custom.root, Some("custom"), "Custom");
        custom_page.set_icon_name(Some("media-playback-start-symbolic"));
        let daemon_stack_page = view_stack.add_titled(&daemon.root, Some("daemon"), "Daemon");
        daemon_stack_page.set_icon_name(Some("system-run-symbolic"));

        // --- Disconnected status page ------------------------------------
        let status_page = adw::StatusPage::new();
        status_page.set_icon_name(Some("keyboard-brightness-symbolic"));
        status_page.set_title("Daemon Not Running");
        status_page.set_description(Some("The background service that drives the keyboard lighting is not running."));

        let start_button = gtk::Button::with_label("Start Daemon");
        start_button.add_css_class("suggested-action");
        start_button.add_css_class("pill");
        start_button.set_halign(gtk::Align::Center);
        let start_sender = sender.clone();
        start_button.connect_clicked(move |_| {
            start_sender.input(AppMsg::StartDaemonRequested);
        });
        status_page.set_child(Some(&start_button));

        // --- Shell ---------------------------------------------------------
        let content_stack = gtk::Stack::new();
        content_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        content_stack.add_named(&view_stack, Some("main"));
        content_stack.add_named(&status_page, Some("disconnected"));
        content_stack.set_visible_child_name("disconnected");

        let switcher = adw::ViewSwitcher::new();
        switcher.set_policy(adw::ViewSwitcherPolicy::Wide);
        switcher.set_stack(Some(&view_stack));

        let header_bar = adw::HeaderBar::new();
        header_bar.set_title_widget(Some(&switcher));

        let permission_banner = adw::Banner::new("");
        permission_banner.set_revealed(false);

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header_bar);
        toolbar_view.add_top_bar(&permission_banner);
        toolbar_view.set_content(Some(&content_stack));

        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_child(Some(&toolbar_view));

        root.set_content(Some(&toast_overlay));

        let widgets = AppWidgets {
            toast_overlay,
            permission_banner,
            content_stack,
            lighting,
            profiles,
            custom,
            daemon,
        };

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AppMsg::Ipc(update) => self.handle_ipc_update(update, &sender),

            AppMsg::EffectSelected(index) => {
                let Some(selected) = effect_by_index(index) else {
                    return;
                };
                // Discriminant-only equality: reselecting the same effect
                // (even with different inner settings) is a no-op.
                if selected == self.profile.effect {
                    return;
                }
                self.profile.effect = selected;
                self.push_profile();
            }
            AppMsg::ZoneColorPicked { zone_index, color } => {
                if zone_index >= self.profile.rgb_zones.len() {
                    return;
                }
                if self.profile.rgb_zones[zone_index].rgb == color {
                    return;
                }
                self.profile.rgb_zones[zone_index].rgb = color;
                self.push_profile();
            }
            AppMsg::GlobalColorDialogRequested => {
                show_global_color_dialog(self.profile.rgb_zones[0].rgb, &sender);
            }
            AppMsg::GlobalColorPicked { color } => {
                let mut changed = false;
                for zone in &mut self.profile.rgb_zones {
                    if zone.rgb != color {
                        zone.rgb = color;
                        changed = true;
                    }
                }
                if changed {
                    self.push_profile();
                }
            }
            AppMsg::SpeedPicked { speed } => {
                if self.profile.speed == speed {
                    return;
                }
                self.profile.speed = speed;
                self.push_profile();
            }
            AppMsg::BrightnessPicked { high } => {
                let brightness = if high { Brightness::High } else { Brightness::Low };
                if self.profile.brightness == brightness {
                    return;
                }
                self.profile.brightness = brightness;
                self.push_profile();
            }
            AppMsg::DirectionPicked { index } => {
                let direction = if index == 0 { Direction::Left } else { Direction::Right };
                if self.profile.direction == direction {
                    return;
                }
                self.profile.direction = direction;
                self.push_profile();
            }
            AppMsg::SwipeModePicked { index } => {
                let picked_mode = if index == 0 { SwipeMode::Change } else { SwipeMode::Fill };
                let changed = match &mut self.profile.effect {
                    Effects::Swipe { mode, .. } | Effects::SmoothWave { mode, .. } => {
                        if *mode == picked_mode {
                            false
                        } else {
                            *mode = picked_mode;
                            true
                        }
                    }
                    _ => false,
                };
                if changed {
                    self.push_profile();
                }
            }
            AppMsg::CleanWithBlackPicked { clean } => {
                let changed = match &mut self.profile.effect {
                    Effects::Swipe { clean_with_black, .. } | Effects::SmoothWave { clean_with_black, .. } => {
                        if *clean_with_black == clean {
                            false
                        } else {
                            *clean_with_black = clean;
                            true
                        }
                    }
                    _ => false,
                };
                if changed {
                    self.push_profile();
                }
            }
            AppMsg::AmbientFpsPicked { fps: picked_fps } => {
                let changed = match &mut self.profile.effect {
                    Effects::AmbientLight { fps, .. } => {
                        if *fps == picked_fps {
                            false
                        } else {
                            *fps = picked_fps;
                            true
                        }
                    }
                    _ => false,
                };
                if changed {
                    self.push_profile();
                }
            }
            AppMsg::AmbientSaturationPicked { saturation: picked } => {
                let changed = match &mut self.profile.effect {
                    Effects::AmbientLight { saturation_boost, .. } => {
                        if (*saturation_boost - picked).abs() < 0.001 {
                            false
                        } else {
                            *saturation_boost = picked;
                            true
                        }
                    }
                    _ => false,
                };
                if changed {
                    self.push_profile();
                }
            }

            AppMsg::ProfileActivated { name } => {
                self.ipc.send(Request::SwitchProfile { name });
            }
            AppMsg::ProfileDeleted { name } => {
                self.ipc.send(Request::DeleteProfile { name });
            }
            AppMsg::SaveProfileDialogRequested => {
                show_save_profile_dialog(&sender);
            }
            AppMsg::SaveProfileConfirmed { name } => {
                if name.is_empty() {
                    self.queue_toast("Profile name cannot be empty");
                    return;
                }
                self.profile.name = Some(name);
                self.ipc.send(Request::AddProfile { profile: self.profile.clone() });
                self.push_profile();
            }

            AppMsg::CustomEffectPlayed { name } => {
                let Some(state) = &self.state else {
                    return;
                };
                let mut found = None;
                for effect in &state.custom_effects {
                    if effect.name.as_deref() == Some(name.as_str()) {
                        found = Some(effect.clone());
                        break;
                    }
                }
                match found {
                    Some(effect) => self.ipc.send(Request::PlayCustomEffect { effect }),
                    None => self.queue_toast(&format!("Custom effect “{name}” not found")),
                }
            }
            AppMsg::CustomEffectDeleted { name } => {
                self.ipc.send(Request::DeleteCustomEffect { name });
            }
            AppMsg::CustomEffectStopped => {
                self.ipc.send(Request::StopCustomEffect);
            }
            AppMsg::CustomEffectFileRequested => {
                show_custom_effect_file_dialog(&sender);
            }
            AppMsg::CustomEffectFileChosen { path } => {
                self.load_custom_effect_file(&path);
            }

            AppMsg::StartDaemonRequested => {
                let deliver_sender = sender.clone();
                daemon_actions::start_daemon(self.autostart_available, move |msg| {
                    deliver_sender.input(msg);
                });
            }
            AppMsg::DaemonRestartRequested => {
                let deliver_sender = sender.clone();
                daemon_actions::restart_daemon(self.autostart_available, move |msg| {
                    deliver_sender.input(msg);
                });
            }
            AppMsg::AutostartToggled { enabled } => {
                if enabled == self.autostart_enabled {
                    return; // Echo from update_view.
                }
                if !self.autostart_available {
                    self.queue_toast("No systemd unit found — install the aurora.service unit first");
                    return;
                }
                if self.autostart_managed {
                    self.queue_toast("Autostart is managed by your home-manager configuration");
                    return;
                }
                self.autostart_enabled = enabled;
                let deliver_sender = sender.clone();
                daemon_actions::set_autostart(enabled, move |msg| {
                    deliver_sender.input(msg);
                });
            }
            AppMsg::AutostartQueried { available, enabled, managed } => {
                self.autostart_available = available;
                self.autostart_enabled = enabled;
                self.autostart_managed = managed;
            }
            AppMsg::ServiceActionFinished { description, error } => {
                if let Some(error) = error {
                    self.queue_toast(&format!("{description}: {error}"));
                }
                // Re-query so the switch reflects reality, not intent.
                let deliver_sender = sender.clone();
                daemon_actions::query_autostart(move |msg| {
                    deliver_sender.input(msg);
                });
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        // --- Connection state ---------------------------------------------
        let visible_child = if self.connected { "main" } else { "disconnected" };
        let current_child = widgets.content_stack.visible_child_name();
        if current_child.as_deref() != Some(visible_child) {
            widgets.content_stack.set_visible_child_name(visible_child);
        }

        // --- Toast ---------------------------------------------------------
        if let Some(text) = self.pending_toast.take() {
            widgets.toast_overlay.add_toast(adw::Toast::new(&text));
        }

        let Some(state) = &self.state else {
            return;
        };

        // --- Keyboard status banner ---------------------------------------
        match &state.keyboard {
            KeyboardStatus::PermissionDenied { .. } => {
                widgets.permission_banner.set_title("Keyboard access denied — install the udev rule, then replug or reboot");
                widgets.permission_banner.set_revealed(true);
            }
            KeyboardStatus::Searching => {
                widgets.permission_banner.set_title("Looking for a supported keyboard…");
                widgets.permission_banner.set_revealed(true);
            }
            KeyboardStatus::Error { message } => {
                widgets.permission_banner.set_title(&format!("Keyboard error: {message}"));
                widgets.permission_banner.set_revealed(true);
            }
            KeyboardStatus::Connected => {
                widgets.permission_banner.set_revealed(false);
            }
        }

        // --- Lighting page -------------------------------------------------
        self.sync_lighting_page(&widgets.lighting);

        // Lighting edits are disabled while a custom effect plays.
        let lighting_enabled = state.custom_effect_playing.is_none();
        if widgets.lighting.root.is_sensitive() != lighting_enabled {
            widgets.lighting.root.set_sensitive(lighting_enabled);
        }

        // --- Other pages ----------------------------------------------------
        widgets.profiles.sync(&state.profiles, self.profile.name.as_deref(), &sender);
        widgets.custom.sync(&state.custom_effects, state.custom_effect_playing.as_deref(), &sender);

        // --- Daemon page ----------------------------------------------------
        let status_text = format!("Running (v{})", state.version);
        if widgets.daemon.status_row.subtitle().as_deref() != Some(status_text.as_str()) {
            widgets.daemon.status_row.set_subtitle(&status_text);
        }

        if widgets.daemon.autostart_row.is_active() != self.autostart_enabled {
            widgets.daemon.autostart_row.set_active(self.autostart_enabled);
        }
        let autostart_subtitle = if !self.autostart_available {
            "No systemd unit installed"
        } else if self.autostart_managed {
            "Managed by your home-manager configuration"
        } else {
            "Enable the systemd user service"
        };
        if widgets.daemon.autostart_row.subtitle().as_deref() != Some(autostart_subtitle) {
            widgets.daemon.autostart_row.set_subtitle(autostart_subtitle);
        }
        let switch_sensitive = self.autostart_available && !self.autostart_managed;
        if widgets.daemon.autostart_row.is_sensitive() != switch_sensitive {
            widgets.daemon.autostart_row.set_sensitive(switch_sensitive);
        }
    }
}

impl App {
    fn handle_ipc_update(&mut self, update: IpcUpdate, sender: &ComponentSender<Self>) {
        match update {
            IpcUpdate::Connected => {
                self.connected = true;
                let deliver_sender = sender.clone();
                daemon_actions::query_autostart(move |msg| {
                    deliver_sender.input(msg);
                });
            }
            IpcUpdate::Disconnected => {
                self.connected = false;
                self.state = None;
            }
            IpcUpdate::State(state) => {
                self.profile = state.current.clone();
                self.state = Some(*state);
            }
            IpcUpdate::RequestFailed(message) => {
                self.queue_toast(&message);
            }
        }
    }

    /// Send the local profile to the daemon; the resulting StateChanged
    /// event confirms it (and updates every other connected client).
    fn push_profile(&self) {
        self.ipc.send(Request::SetProfile { profile: self.profile.clone() });
    }

    fn queue_toast(&self, text: &str) {
        self.pending_toast.set(Some(text.to_string()));
    }

    fn load_custom_effect_file(&self, path: &std::path::Path) {
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) => {
                self.queue_toast(&format!("Could not read {}: {error}", path.display()));
                return;
            }
        };

        let mut effect: aurora_protocol::custom_effect::CustomEffect = match serde_json::from_str(&contents) {
            Ok(effect) => effect,
            Err(error) => {
                self.queue_toast(&format!("Not a valid custom effect file: {error}"));
                return;
            }
        };

        // Name it after the file when the file itself has no name, so it
        // can be saved and listed.
        if effect.name.is_none() {
            let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned());
            effect.name = stem;
        }

        self.ipc.send(Request::AddCustomEffect { effect: effect.clone() });
        self.ipc.send(Request::PlayCustomEffect { effect });
    }

    fn sync_lighting_page(&self, page: &lighting::LightingPage) {
        // Effect combo.
        if let Some(index) = effect_index(self.profile.effect) {
            if page.effect_row.selected() != index as u32 {
                page.effect_row.set_selected(index as u32);
            }
        }

        // Zone colors.
        for (button, zone) in page.zone_buttons.iter().zip(self.profile.rgb_zones.iter()) {
            let shown = lighting::rgba_to_bytes(&button.rgba());
            if shown != zone.rgb {
                button.set_rgba(&lighting::bytes_to_rgba(zone.rgb));
            }
        }

        // Options.
        let shown_speed = page.speed_row.value() as u8;
        if shown_speed != self.profile.speed {
            page.speed_row.set_value(f64::from(self.profile.speed));
        }

        let high = self.profile.brightness == Brightness::High;
        if page.brightness_row.is_active() != high {
            page.brightness_row.set_active(high);
        }

        let direction_index: u32 = match self.profile.direction {
            Direction::Left => 0,
            Direction::Right => 1,
        };
        if page.direction_row.selected() != direction_index {
            page.direction_row.set_selected(direction_index);
        }

        // Sensitivity follows what the effect supports.
        let takes_colors = self.profile.effect.takes_color_array();
        if page.colors_group.is_sensitive() != takes_colors {
            page.colors_group.set_sensitive(takes_colors);
        }
        let takes_speed = self.profile.effect.takes_speed();
        if page.speed_row.is_sensitive() != takes_speed {
            page.speed_row.set_sensitive(takes_speed);
        }
        let takes_direction = self.profile.effect.takes_direction();
        if page.direction_row.is_sensitive() != takes_direction {
            page.direction_row.set_sensitive(takes_direction);
        }

        // Per-effect groups.
        let is_ambient = matches!(self.profile.effect, Effects::AmbientLight { .. });
        if page.ambient_group.is_visible() != is_ambient {
            page.ambient_group.set_visible(is_ambient);
        }
        if let Effects::AmbientLight { fps, saturation_boost } = self.profile.effect {
            let shown_fps = page.fps_row.value() as u8;
            if shown_fps != fps {
                page.fps_row.set_value(f64::from(fps));
            }
            let shown_saturation = page.saturation_row.value() as f32;
            if (shown_saturation - saturation_boost).abs() > 0.001 {
                page.saturation_row.set_value(f64::from(saturation_boost));
            }
        }

        let is_swipe = matches!(self.profile.effect, Effects::Swipe { .. } | Effects::SmoothWave { .. });
        if page.swipe_group.is_visible() != is_swipe {
            page.swipe_group.set_visible(is_swipe);
        }
        if let Effects::Swipe { mode, clean_with_black } | Effects::SmoothWave { mode, clean_with_black } = self.profile.effect {
            let mode_index: u32 = match mode {
                SwipeMode::Change => 0,
                SwipeMode::Fill => 1,
            };
            if page.swipe_mode_row.selected() != mode_index {
                page.swipe_mode_row.set_selected(mode_index);
            }
            if page.clean_row.is_active() != clean_with_black {
                page.clean_row.set_active(clean_with_black);
            }
            let clean_sensitive = mode == SwipeMode::Fill;
            if page.clean_row.is_sensitive() != clean_sensitive {
                page.clean_row.set_sensitive(clean_sensitive);
            }
        }

        // Preview.
        let mut preview_colors: [[u8; 3]; 4] = [[0; 3]; 4];
        for (target, zone) in preview_colors.iter_mut().zip(self.profile.rgb_zones.iter()) {
            if zone.enabled {
                *target = zone.rgb;
            }
        }
        page.preview.set_colors(preview_colors);
    }
}

/// Effects in `Effects::iter()` order with usable defaults for the
/// field-carrying variants (the iterator yields zeroed fields).
fn effect_by_index(index: usize) -> Option<Effects> {
    let effect = Effects::iter().nth(index)?;

    let with_defaults = match effect {
        Effects::AmbientLight { .. } => Effects::AmbientLight {
            fps: 30,
            saturation_boost: 0.0,
        },
        Effects::SmoothWave { .. } => Effects::SmoothWave {
            mode: SwipeMode::Change,
            clean_with_black: false,
        },
        Effects::Swipe { .. } => Effects::Swipe {
            mode: SwipeMode::Change,
            clean_with_black: false,
        },
        other => other,
    };

    Some(with_defaults)
}

fn effect_index(effect: Effects) -> Option<usize> {
    for (index, candidate) in Effects::iter().enumerate() {
        // Discriminant-only equality.
        if candidate == effect {
            return Some(index);
        }
    }
    None
}

fn show_save_profile_dialog(sender: &ComponentSender<App>) {
    let Some(window) = relm4::main_application().active_window() else {
        return;
    };

    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Profile name"));
    entry.set_margin_top(6);

    let dialog = adw::AlertDialog::new(Some("Save Profile"), Some("Save the current lighting as a profile."));
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("save", "Save");
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");

    let dialog_sender = sender.clone();
    dialog.connect_response(None, move |dialog, response| {
        if response == "save" {
            let name = dialog
                .extra_child()
                .and_downcast::<gtk::Entry>()
                .map(|entry| entry.text().to_string())
                .unwrap_or_default();
            dialog_sender.input(AppMsg::SaveProfileConfirmed { name });
        }
    });

    dialog.present(Some(&window));
}

fn show_global_color_dialog(initial_color: [u8; 3], sender: &ComponentSender<App>) {
    let Some(window) = relm4::main_application().active_window() else {
        return;
    };

    let dialog = gtk::ColorDialog::new();
    dialog.set_with_alpha(false);

    let dialog_sender = sender.clone();
    dialog.choose_rgba(
        Some(&window),
        Some(&lighting::bytes_to_rgba(initial_color)),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            let rgba = match result {
                Ok(rgba) => rgba,
                Err(_) => return, // Dismissed.
            };
            let color = lighting::rgba_to_bytes(&rgba);
            dialog_sender.input(AppMsg::GlobalColorPicked { color });
        },
    );
}

fn show_custom_effect_file_dialog(sender: &ComponentSender<App>) {
    let Some(window) = relm4::main_application().active_window() else {
        return;
    };

    let json_filter = gtk::FileFilter::new();
    json_filter.set_name(Some("Custom effect files (JSON)"));
    json_filter.add_pattern("*.json");

    let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&json_filter);

    let dialog = gtk::FileDialog::builder().title("Load Custom Effect").filters(&filters).build();

    let dialog_sender = sender.clone();
    dialog.open(Some(&window), None::<&gtk::gio::Cancellable>, move |result| {
        let file = match result {
            Ok(file) => file,
            Err(_) => return, // Dismissed.
        };
        if let Some(path) = file.path() {
            dialog_sender.input(AppMsg::CustomEffectFileChosen { path });
        }
    });
}
