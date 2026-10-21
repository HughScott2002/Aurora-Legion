//! Root component. Written as a manual `SimpleComponent` (no view! macro)
//! so every widget update and every message is visible as plain Rust:
//! signals send `AppMsg`, `update` mutates the model and talks to the
//! daemon, `update_view` syncs widgets from the model with explicit
//! compares (which is also what stops signal echo loops).

use std::{
    cell::Cell,
    time::{Duration, Instant},
};

use aurora_protocol::{
    effects::{Brightness, Direction, Effects, SwipeMode},
    ipc::{DaemonState, KeyboardStatus, Request, SlotSelection, SubsystemState},
    profile::{Lighting, COLOR_CHANNELS_PER_ZONE, SLOT_COUNT},
};
use relm4::{
    adw::{self, prelude::*},
    gtk, ComponentParts, ComponentSender, SimpleComponent,
};

use crate::{
    daemon_actions::{self, ServiceAction},
    ipc::{self, IpcHandle, IpcUpdate},
    links,
    pages::{custom_effects, lighting, profiles, settings},
    problem_dialog,
    problems::{self, Help, Problem, RECOVERY_HELP_DELAY, RECOVERY_NOTICE_DELAY},
};

const WINDOW_DEFAULT_WIDTH: i32 = 640;
const WINDOW_DEFAULT_HEIGHT: i32 = 720;
const SERVICE_ACTION_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_EFFECT_FILE_BYTES: u64 = aurora_protocol::ipc::MAX_LINE_BYTES as u64;

/// How long a locally edited value keeps precedence over daemon snapshots.
///
/// Every edit is sent immediately and comes back in a broadcast, but a
/// snapshot generated just before the edit landed would otherwise overwrite
/// the control the user is still dragging. Outside this window the daemon
/// is always the source of truth, which is what keeps the preview, the
/// pickers and the slot rows from drifting apart.
const LOCAL_EDIT_GRACE: Duration = Duration::from_millis(400);

/// The red the preview draws while the low-battery alert holds the
/// keyboard. It matches the colour the daemon writes; the preview has
/// never drawn brightness for any effect, so it does not draw it here
/// either.
const BATTERY_ALERT_PREVIEW_COLOR: [u8; 3] = [255, 0, 0];

pub struct App {
    connected: bool,
    state: Option<DaemonState>,
    /// Edit buffer for the active slot's lighting. Adopted from every
    /// daemon snapshot except while [`LOCAL_EDIT_GRACE`] is running.
    lighting: Lighting,
    /// When the last local edit happened, if it was recent.
    last_local_edit_at: Option<Instant>,
    ipc: IpcHandle,

    autostart_available: bool,
    autostart_enabled: bool,
    autostart_managed: bool,

    /// A start-daemon action is in flight; the button shows it and blocks
    /// double clicks until the daemon connects or the action reports back.
    pending_service: Option<ServiceAction>,
    service_request_id: u64,
    service_deadline: Option<Instant>,
    recovery_started_at: Option<Instant>,
    status_refresh: Option<gtk::glib::Source>,
    problem: Option<Problem>,
    connection_details: String,
    /// The keyboard connected at least once this session. Distinguishes
    /// "unplugged just now" from "never found one" in the banner.
    keyboard_seen: bool,

    /// Toast queued by update(), shown and cleared by update_view().
    pending_toast: Cell<Option<String>>,

    /// Set when the daemon speaks a protocol this build cannot talk to.
    /// The connection worker has stopped by then, so this is permanent
    /// until the app restarts against a matching daemon.
    incompatible: Option<String>,
}

#[derive(Debug)]
pub enum AppMsg {
    Ipc(IpcUpdate),

    EffectSelected(usize),
    ZoneColorPicked {
        zone_index: usize,
        color: [u8; 3],
    },
    GlobalColorDialogRequested,
    GlobalColorPicked {
        color: [u8; 3],
    },
    SpeedPicked {
        speed: u8,
    },
    BrightnessPicked {
        high: bool,
    },
    DirectionPicked {
        index: usize,
    },
    SwipeModePicked {
        index: usize,
    },
    CleanWithBlackPicked {
        clean: bool,
    },
    AmbientFpsPicked {
        fps: u8,
    },
    AmbientSaturationPicked {
        saturation: f32,
    },

    SlotSelected {
        slot: SlotSelection,
    },

    ProfileActivated {
        name: String,
    },
    ProfileDeleted {
        name: String,
    },
    SaveProfileDialogRequested,
    SaveProfileConfirmed {
        name: String,
    },

    CustomEffectPlayed {
        name: String,
    },
    CustomEffectDeleted {
        name: String,
    },
    CustomEffectStopped,
    CustomEffectFileRequested,
    CustomEffectFileChosen {
        path: std::path::PathBuf,
    },

    StartDaemonRequested,
    DaemonRestartRequested,
    AutostartToggled {
        enabled: bool,
    },
    BatteryAlertToggled {
        enabled: bool,
    },
    AutostartQueried {
        available: bool,
        enabled: bool,
        managed: bool,
    },
    ServiceActionFinished {
        request_id: u64,
        action: ServiceAction,
        error: Option<String>,
    },
    CustomEffectLoaded(Result<aurora_protocol::custom_effect::CustomEffect, String>),
    RefreshStatus,
    ProblemAction,
    ShowProblemDetails,
}

pub struct AppWidgets {
    toast_overlay: adw::ToastOverlay,
    status_page: adw::StatusPage,
    problem_banner: adw::Banner,
    content_stack: gtk::Stack,
    start_button: gtk::Button,

    lighting: lighting::LightingPage,
    profiles: profiles::ProfilesPage,
    custom: custom_effects::CustomEffectsPage,
    settings: settings::SettingsPage,
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(source) = self.status_refresh.take() {
            source.destroy();
        }
    }
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

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // --- Connection worker -------------------------------------------
        // input_sender().send rather than input(): the latter logs and
        // discards the failure, so the worker could never tell that the
        // window had closed and kept reconnecting into a dead runtime.
        let ipc_sender = sender.clone();
        let ipc =
            ipc::spawn(move |update| ipc_sender.input_sender().send(AppMsg::Ipc(update)).is_ok());

        let model = App {
            connected: false,
            state: None,
            lighting: Lighting::default(),
            last_local_edit_at: None,
            ipc,
            autostart_available: false,
            autostart_enabled: false,
            autostart_managed: false,
            pending_service: None,
            service_request_id: 0,
            service_deadline: None,
            recovery_started_at: None,
            status_refresh: None,
            problem: None,
            connection_details: String::new(),
            keyboard_seen: false,
            pending_toast: Cell::new(None),
            incompatible: None,
        };

        // Query systemd availability now, not on first connect: the Start
        // Daemon button on the disconnected page must know whether to go
        // through systemctl before the daemon has ever answered.
        let autostart_sender = sender.clone();
        daemon_actions::query_autostart(move |msg| {
            autostart_sender.input(msg);
        });

        // --- Pages --------------------------------------------------------
        let lighting = lighting::build(&sender);
        let profiles = profiles::build(&sender);
        let custom = custom_effects::build(&sender);
        let settings = settings::build(&sender);

        let view_stack = adw::ViewStack::new();
        let lighting_page = view_stack.add_titled(&lighting.root, Some("lighting"), "Lighting");
        lighting_page.set_icon_name(Some("keyboard-brightness-symbolic"));
        let profiles_page = view_stack.add_titled(&profiles.root, Some("profiles"), "Profiles");
        profiles_page.set_icon_name(Some("view-list-bullet-symbolic"));
        let custom_page = view_stack.add_titled(&custom.root, Some("custom"), "Custom");
        custom_page.set_icon_name(Some("media-playback-start-symbolic"));
        let settings_stack_page =
            view_stack.add_titled(&settings.root, Some("settings"), "Settings");
        settings_stack_page.set_icon_name(Some("preferences-system-symbolic"));

        // --- Disconnected status page ------------------------------------
        let status_page = adw::StatusPage::new();
        status_page.set_icon_name(Some("keyboard-brightness-symbolic"));
        status_page.set_title("Connecting to Aurora…");
        status_page.set_description(Some("Waiting for the lighting service to connect."));

        let start_button = gtk::Button::with_label("Start Service");
        start_button.add_css_class("suggested-action");
        start_button.add_css_class("pill");
        start_button.set_halign(gtk::Align::Center);
        let start_sender = sender.clone();
        start_button.connect_clicked(move |_| {
            start_sender.input(AppMsg::StartDaemonRequested);
        });
        let status_actions = gtk::Box::new(gtk::Orientation::Vertical, 12);
        status_actions.set_halign(gtk::Align::Center);
        status_actions.append(&start_button);
        let help_button = gtk::Button::with_label("Connection Help");
        help_button.add_css_class("flat");
        let help_sender = sender.clone();
        help_button.connect_clicked(move |_| help_sender.input(AppMsg::ShowProblemDetails));
        status_actions.append(&help_button);
        status_page.set_child(Some(&status_actions));

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

        // Primary menu (HIG: every app has one, with About).
        let menu = gtk::gio::Menu::new();
        menu.append(Some("Connection Help"), Some("app.connection-help"));
        menu.append(Some("About Aurora"), Some("app.about"));
        let menu_button = gtk::MenuButton::new();
        menu_button.set_icon_name("open-menu-symbolic");
        menu_button.set_tooltip_text(Some("Main Menu"));
        menu_button.set_menu_model(Some(&menu));
        header_bar.pack_end(&menu_button);

        let help_action = gtk::gio::SimpleAction::new("connection-help", None);
        let help_sender = sender.clone();
        help_action.connect_activate(move |_, _| help_sender.input(AppMsg::ShowProblemDetails));
        relm4::main_application().add_action(&help_action);

        let about_action = gtk::gio::SimpleAction::new("about", None);
        about_action.connect_activate(|_, _| {
            show_about_dialog();
        });
        relm4::main_application().add_action(&about_action);

        let problem_banner = adw::Banner::new("");
        problem_banner.set_revealed(false);
        // Recoverable interruptions need no action until help is useful.
        let problem_sender = sender.clone();
        problem_banner.connect_button_clicked(move |_| {
            problem_sender.input(AppMsg::ProblemAction);
        });

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header_bar);
        toolbar_view.add_top_bar(&problem_banner);
        toolbar_view.set_content(Some(&content_stack));

        // Narrow windows: the wide switcher does not fit in the header, so
        // a breakpoint moves page switching to a bottom bar (the standard
        // adaptive libadwaita pattern).
        let switcher_bar = adw::ViewSwitcherBar::new();
        switcher_bar.set_stack(Some(&view_stack));
        toolbar_view.add_bottom_bar(&switcher_bar);

        let narrow_title = adw::WindowTitle::new("Aurora", "");
        let narrow_condition = adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            550.0,
            adw::LengthUnit::Sp,
        );
        let narrow_breakpoint = adw::Breakpoint::new(narrow_condition);
        narrow_breakpoint.add_setter(&switcher_bar, "reveal", Some(&true.to_value()));
        narrow_breakpoint.add_setter(&header_bar, "title-widget", Some(&narrow_title.to_value()));
        root.add_breakpoint(narrow_breakpoint);

        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_child(Some(&toolbar_view));

        root.set_content(Some(&toast_overlay));

        let widgets = AppWidgets {
            toast_overlay,
            status_page,
            problem_banner,
            content_stack,
            start_button,
            lighting,
            profiles,
            custom,
            settings,
        };

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AppMsg::Ipc(update) => self.handle_ipc_update(update, &sender),

            AppMsg::EffectSelected(index) => {
                // Decoded against the list the user was looking at, which
                // is why the currently active effect is passed in.
                let Some(selected) =
                    effect_by_index(self.battery_available(), self.lighting.effect, index)
                else {
                    return;
                };
                // Reselecting the same effect is a no-op even when its
                // inner settings differ; that is what same_variant is for.
                // Structural equality here would refire on every fps tweak.
                if selected.same_variant(self.lighting.effect) {
                    return;
                }
                self.lighting.effect = selected;
                self.push_lighting();
            }
            AppMsg::ZoneColorPicked { zone_index, color } => {
                if zone_index >= self.lighting.rgb_zones.len() {
                    return;
                }
                if self.lighting.rgb_zones[zone_index].rgb == color {
                    return;
                }
                self.lighting.rgb_zones[zone_index].rgb = color;
                self.push_lighting();
            }
            AppMsg::GlobalColorDialogRequested => {
                show_global_color_dialog(self.lighting.rgb_zones[0].rgb, &sender);
            }
            AppMsg::GlobalColorPicked { color } => {
                let mut changed = false;
                for zone in &mut self.lighting.rgb_zones {
                    if zone.rgb != color {
                        zone.rgb = color;
                        changed = true;
                    }
                }
                if changed {
                    self.push_lighting();
                }
            }
            AppMsg::SpeedPicked { speed } => {
                if self.lighting.speed == speed {
                    return;
                }
                self.lighting.speed = speed;
                self.push_lighting();
            }
            AppMsg::BrightnessPicked { high } => {
                let brightness = if high {
                    Brightness::High
                } else {
                    Brightness::Low
                };
                if self.lighting.brightness == brightness {
                    return;
                }
                self.lighting.brightness = brightness;
                self.push_lighting();
            }
            AppMsg::DirectionPicked { index } => {
                let direction = if index == 0 {
                    Direction::Left
                } else {
                    Direction::Right
                };
                if self.lighting.direction == direction {
                    return;
                }
                self.lighting.direction = direction;
                self.push_lighting();
            }
            AppMsg::SwipeModePicked { index } => {
                let picked_mode = if index == 0 {
                    SwipeMode::Change
                } else {
                    SwipeMode::Fill
                };
                let changed = match &mut self.lighting.effect {
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
                    self.push_lighting();
                }
            }
            AppMsg::CleanWithBlackPicked { clean } => {
                let changed = match &mut self.lighting.effect {
                    Effects::Swipe {
                        clean_with_black, ..
                    }
                    | Effects::SmoothWave {
                        clean_with_black, ..
                    } => {
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
                    self.push_lighting();
                }
            }
            AppMsg::AmbientFpsPicked { fps: picked_fps } => {
                let changed = match &mut self.lighting.effect {
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
                    self.push_lighting();
                }
            }
            AppMsg::AmbientSaturationPicked { saturation: picked } => {
                let changed = match &mut self.lighting.effect {
                    Effects::AmbientLight {
                        saturation_boost, ..
                    } => {
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
                    self.push_lighting();
                }
            }

            AppMsg::SlotSelected { slot } => {
                let Some(state) = &self.state else {
                    return;
                };
                if state.active_slot == slot {
                    return; // Echo from update_view, or a second click.
                }
                // No optimistic update: the daemon owns which slot is live,
                // and its broadcast is what moves the rows.
                self.send_request(Request::SelectSlot { slot });
            }

            AppMsg::ProfileActivated { name } => {
                self.send_request(Request::SwitchProfile { name });
            }
            AppMsg::ProfileDeleted { name } => {
                self.send_request(Request::DeleteProfile { name });
            }
            AppMsg::SaveProfileDialogRequested => {
                show_save_profile_dialog(&sender);
            }
            AppMsg::SaveProfileConfirmed { name } => {
                if name.is_empty() {
                    return;
                }
                let Some(state) = &self.state else {
                    return;
                };
                let mut profile = state.current.clone();
                profile.name = Some(name);
                self.send_request(Request::AddProfile { profile });
            }

            AppMsg::CustomEffectPlayed { name } => {
                // By name: the daemon holds the body, so it never travels
                // back to the process that stored it.
                self.send_request(Request::PlayCustomEffectByName { name });
            }
            AppMsg::CustomEffectDeleted { name } => {
                self.send_request(Request::DeleteCustomEffect { name });
            }
            AppMsg::CustomEffectStopped => {
                self.send_request(Request::StopCustomEffect);
            }
            AppMsg::CustomEffectFileRequested => {
                show_custom_effect_file_dialog(&sender);
            }
            AppMsg::CustomEffectFileChosen { path } => {
                let load_sender = sender.clone();
                std::thread::spawn(move || {
                    load_sender.input(AppMsg::CustomEffectLoaded(read_custom_effect_file(&path)));
                });
            }
            AppMsg::CustomEffectLoaded(result) => match result {
                Ok(effect) => {
                    self.problem = None;
                    self.send_request(Request::AddCustomEffect {
                        effect: effect.clone(),
                    });
                    self.send_request(Request::PlayCustomEffect { effect });
                }
                Err(details) => self.problem = Some(Problem::file(details)),
            },
            AppMsg::RefreshStatus => {
                self.status_refresh = None;
                if self
                    .service_deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
                {
                    let title = self
                        .pending_service
                        .map(ServiceAction::failure_title)
                        .unwrap_or("The lighting service did not respond.");
                    self.pending_service = None;
                    self.service_deadline = None;
                    self.problem = Some(Problem {
                        title,
                        description:
                            "The action took too long. You can try again or open connection help.",
                        details: "The service action exceeded its 15 second deadline.".to_string(),
                        help: Help::Connection,
                    });
                }
            }
            AppMsg::ProblemAction => {
                if let Some(problem) = self.visible_problem() {
                    if problem.help == Help::EffectFile {
                        show_custom_effect_file_dialog(&sender);
                    } else {
                        problem_dialog::show(&problem);
                    }
                }
            }
            AppMsg::ShowProblemDetails => {
                let problem = self.help_problem();
                problem_dialog::show(&problem);
            }

            AppMsg::StartDaemonRequested => {
                if self.incompatible.is_some() {
                    problem_dialog::show(&self.help_problem());
                } else if let Some(request_id) = self.begin_service_action(ServiceAction::Start) {
                    let deliver_sender = sender.clone();
                    daemon_actions::start_daemon(
                        self.autostart_available,
                        request_id,
                        move |msg| {
                            deliver_sender.input(msg);
                        },
                    );
                }
            }
            AppMsg::DaemonRestartRequested => {
                if let Some(request_id) = self.begin_service_action(ServiceAction::Restart) {
                    let deliver_sender = sender.clone();
                    daemon_actions::restart_daemon(
                        self.autostart_available,
                        request_id,
                        move |msg| {
                            deliver_sender.input(msg);
                        },
                    );
                }
            }
            AppMsg::AutostartToggled { enabled } => {
                if enabled == self.autostart_enabled {
                    return; // Echo from update_view.
                }
                if !self.autostart_available {
                    self.problem = Some(Problem {
                        title: "Start at Login needs to be set up first.",
                        description: "Open the setup guide to enable this option for your system.",
                        details: "No aurora.service user unit was found.".to_string(),
                        help: Help::Setup,
                    });
                    return;
                }
                if self.autostart_managed {
                    self.problem = Some(Problem {
                        title: "Start at Login is managed by your system configuration.",
                        description: "Change this option in your system configuration, \
                                      then reopen Aurora.",
                        details: "The user service is managed by Home Manager.".to_string(),
                        help: Help::Setup,
                    });
                    return;
                }
                let action = if enabled {
                    ServiceAction::EnableAutostart
                } else {
                    ServiceAction::DisableAutostart
                };
                let Some(request_id) = self.begin_service_action(action) else {
                    return;
                };
                self.autostart_enabled = enabled;
                let deliver_sender = sender.clone();
                daemon_actions::set_autostart(enabled, request_id, move |msg| {
                    deliver_sender.input(msg);
                });
            }
            AppMsg::BatteryAlertToggled { enabled } => {
                // The daemon owns this setting, so the switch reports the
                // change and waits for the state broadcast to confirm it,
                // exactly like every other lighting control.
                let Some(state) = &self.state else {
                    return;
                };
                if state.battery_alert == enabled {
                    return; // Echo from update_view.
                }
                self.send_request(Request::SetBatteryAlert { enabled });
            }
            AppMsg::AutostartQueried {
                available,
                enabled,
                managed,
            } => {
                self.autostart_available = available;
                self.autostart_enabled = enabled;
                self.autostart_managed = managed;
            }
            AppMsg::ServiceActionFinished {
                request_id,
                action,
                error,
            } => {
                if request_id != self.service_request_id || self.pending_service.is_none() {
                    return;
                }
                if let Some(details) = error {
                    self.pending_service = None;
                    self.service_deadline = None;
                    self.problem = Some(Problem {
                        title: action.failure_title(),
                        description: "Try again. If this continues, open the connection help.",
                        details,
                        help: Help::Connection,
                    });
                } else if matches!(
                    action,
                    ServiceAction::EnableAutostart | ServiceAction::DisableAutostart
                ) {
                    self.pending_service = None;
                    self.service_deadline = None;
                }
                // Re-query so the switch reflects reality, not intent.
                let deliver_sender = sender.clone();
                daemon_actions::query_autostart(move |msg| {
                    deliver_sender.input(msg);
                });
            }
        }
        self.schedule_status_refresh(&sender);
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        // --- Connection state ---------------------------------------------
        let (title, description, start_label) = if self.incompatible.is_some() {
            (
                "Aurora needs updating",
                "Some installed parts of Aurora do not match. Open the update steps to reconnect.",
                "Show Update Steps",
            )
        } else if self.pending_service.is_some() {
            (
                "Connecting to the lighting service…",
                "Waiting for Aurora to connect.",
                "Working…",
            )
        } else {
            (
                "Lighting controls are unavailable",
                "Start the lighting service, or open the connection help.",
                "Start Service",
            )
        };
        if widgets.status_page.title() != title {
            widgets.status_page.set_title(title);
        }
        if widgets.status_page.description().as_deref() != Some(description) {
            widgets.status_page.set_description(Some(description));
        }
        let visible_child = if self.state.is_some() && self.incompatible.is_none() {
            "main"
        } else {
            "disconnected"
        };
        if widgets.content_stack.visible_child_name().as_deref() != Some(visible_child) {
            widgets.content_stack.set_visible_child_name(visible_child);
        }
        if widgets.start_button.label().as_deref() != Some(start_label) {
            widgets.start_button.set_label(start_label);
        }
        let pending = self.pending_service.is_some();
        if widgets.start_button.is_sensitive() == pending {
            widgets.start_button.set_sensitive(!pending);
        }
        if widgets.settings.restart_button.is_sensitive() == pending {
            widgets.settings.restart_button.set_sensitive(!pending);
        }
        for page in [
            &widgets.lighting.root,
            &widgets.profiles.root,
            &widgets.custom.root,
        ] {
            if page.is_sensitive() != self.connected {
                page.set_sensitive(self.connected);
            }
        }
        let problem = self.visible_problem();
        if let Some(problem) = &problem {
            if widgets.problem_banner.title() != problem.title {
                widgets.problem_banner.set_title(problem.title);
            }
            if widgets.problem_banner.button_label().as_deref() != problem.button_label() {
                widgets
                    .problem_banner
                    .set_button_label(problem.button_label());
            }
        }
        if widgets.problem_banner.is_revealed() != problem.is_some() {
            widgets.problem_banner.set_revealed(problem.is_some());
        }

        // --- Toast ---------------------------------------------------------
        if let Some(text) = self.pending_toast.take() {
            widgets.toast_overlay.add_toast(adw::Toast::new(&text));
        }

        let Some(state) = &self.state else {
            return;
        };

        // --- Lighting page -------------------------------------------------
        self.sync_lighting_page(&widgets.lighting);

        // Lighting edits are disabled while a custom effect plays.
        let lighting_enabled = self.connected && state.custom_effect_playing.is_none();
        if widgets.lighting.root.is_sensitive() != lighting_enabled {
            widgets.lighting.root.set_sensitive(lighting_enabled);
        }

        // --- Other pages ----------------------------------------------------
        widgets.profiles.sync(
            &state.profiles,
            state.current.name.as_deref(),
            &state.hotkey,
            &sender,
        );
        widgets.custom.sync(
            &state.custom_effects,
            state.custom_effect_playing.as_deref(),
            &sender,
        );

        // --- Daemon page ----------------------------------------------------
        // Same stale-daemon case the CLI reports: an upgrade replaces the
        // binaries without restarting a running daemon, so the app can be
        // newer than the daemon it is talking to.
        let own_version = env!("CARGO_PKG_VERSION");
        let status_text = if !self.connected {
            "Reconnecting…".to_string()
        } else if state.version == own_version {
            format!("Running (v{})", state.version)
        } else {
            format!(
                "Running (v{}), older than this app (v{own_version}). Restart it to upgrade.",
                state.version
            )
        };
        if widgets.settings.status_row.subtitle().as_deref() != Some(status_text.as_str()) {
            widgets.settings.status_row.set_subtitle(&status_text);
        }

        if widgets.settings.autostart_row.is_active() != self.autostart_enabled {
            widgets
                .settings
                .autostart_row
                .set_active(self.autostart_enabled);
        }
        let autostart_subtitle = if !self.autostart_available {
            "Start at Login is not installed"
        } else if self.autostart_managed {
            "Managed by your system configuration"
        } else {
            "Start the lighting service when you sign in"
        };
        if widgets.settings.autostart_row.subtitle().as_deref() != Some(autostart_subtitle) {
            widgets
                .settings
                .autostart_row
                .set_subtitle(autostart_subtitle);
        }
        let switch_sensitive = self.autostart_available && !self.autostart_managed && !pending;
        if widgets.settings.autostart_row.is_sensitive() != switch_sensitive {
            widgets
                .settings
                .autostart_row
                .set_sensitive(switch_sensitive);
        }

        // The whole group goes on a machine with no battery. Nothing to
        // explain and nothing to switch.
        if widgets.settings.battery_group.get_visible() != state.battery_available {
            widgets
                .settings
                .battery_group
                .set_visible(state.battery_available);
        }
        if widgets.settings.battery_group.is_sensitive() != self.connected {
            widgets.settings.battery_group.set_sensitive(self.connected);
        }
        if widgets.settings.battery_alert_row.is_active() != state.battery_alert {
            widgets
                .settings
                .battery_alert_row
                .set_active(state.battery_alert);
        }
    }
}

impl App {
    fn begin_service_action(&mut self, action: ServiceAction) -> Option<u64> {
        if self.pending_service.is_some() {
            return None;
        }
        self.problem = None;
        self.service_request_id = self.service_request_id.wrapping_add(1);
        self.pending_service = Some(action);
        self.service_deadline = Some(Instant::now() + SERVICE_ACTION_TIMEOUT);
        Some(self.service_request_id)
    }

    fn send_request(&mut self, request: Request) {
        self.problem = None;
        if !self.connected || !self.ipc.send(request) {
            self.problem = Some(Problem::connection(
                "The request could not be queued. The connection may be unavailable or busy."
                    .to_string(),
            ));
        }
    }

    fn visible_problem(&self) -> Option<Problem> {
        if self.incompatible.is_some() {
            return None;
        }
        if let Some(state) = &self.state {
            if let Some(details) = &state.settings_error {
                return Some(Problem {
                    title: "Your changes could not be saved.",
                    description: "Keep Aurora running while you check the details. Your \
                                  current lighting may not be restored next time.",
                    details: details.clone(),
                    help: Help::Details,
                });
            }
        }
        if let Some(problem) = &self.problem {
            return Some(problem.clone());
        }
        let state = self.state.as_ref()?;
        let elapsed = self
            .recovery_started_at
            .map(|start| start.elapsed())
            .unwrap_or_default();
        if !self.connected {
            if elapsed < RECOVERY_NOTICE_DELAY {
                return None;
            }
            let mut problem = Problem::connection(self.connection_details.clone());
            if elapsed < RECOVERY_HELP_DELAY {
                problem.title = "Reconnecting to the lighting service…";
                problem.help = Help::Recovering;
            }
            return Some(problem);
        }
        problems::keyboard_problem(&state.keyboard, self.keyboard_seen, elapsed)
    }

    fn help_problem(&self) -> Problem {
        if let Some(details) = &self.incompatible {
            return Problem::incompatible(details.clone());
        }
        self.visible_problem().unwrap_or_else(|| Problem {
            title: "Connection Help",
            description: "Open the guide for help with keyboard detection, keyboard access, or \
                          starting Aurora.",
            details: self.connection_details.clone(),
            help: Help::Connection,
        })
    }

    fn schedule_status_refresh(&mut self, sender: &ComponentSender<Self>) {
        if let Some(source) = self.status_refresh.take() {
            source.destroy();
        }
        let now = Instant::now();
        let mut deadline = self.service_deadline;
        if let Some(started) = self.recovery_started_at {
            let elapsed = now.saturating_duration_since(started);
            let recovery_deadline = if elapsed < RECOVERY_NOTICE_DELAY {
                Some(started + RECOVERY_NOTICE_DELAY)
            } else if elapsed < RECOVERY_HELP_DELAY {
                Some(started + RECOVERY_HELP_DELAY)
            } else {
                None
            };
            if let Some(recovery_deadline) = recovery_deadline {
                deadline = Some(
                    deadline.map_or(recovery_deadline, |current| current.min(recovery_deadline)),
                );
            }
        }
        let Some(deadline) = deadline else {
            return;
        };
        let refresh_sender = sender.clone();
        let source = gtk::glib::timeout_source_new(
            deadline.saturating_duration_since(now),
            Some("aurora-status-deadline"),
            gtk::glib::Priority::DEFAULT,
            move || {
                refresh_sender.input(AppMsg::RefreshStatus);
                gtk::glib::ControlFlow::Break
            },
        );
        source.attach(None);
        self.status_refresh = Some(source);
    }

    fn handle_ipc_update(&mut self, update: IpcUpdate, sender: &ComponentSender<Self>) {
        match update {
            IpcUpdate::Connected => {}
            IpcUpdate::ConnectionError(details) => self.connection_details = details,
            IpcUpdate::Unresponsive => {
                self.connection_details =
                    "The lighting service did not answer within five seconds.".to_string();
                self.problem = Some(Problem::connection(self.connection_details.clone()));
                self.connected = false;
            }
            IpcUpdate::Disconnected => {
                self.connected = false;
                if self.state.is_some() && self.recovery_started_at.is_none() {
                    self.recovery_started_at = Some(Instant::now());
                }
            }
            IpcUpdate::State(state) => {
                let connection_restored = !self.connected;
                if connection_restored {
                    let deliver_sender = sender.clone();
                    daemon_actions::query_autostart(move |msg| deliver_sender.input(msg));
                }
                self.connected = true;
                if matches!(
                    self.pending_service,
                    Some(ServiceAction::Start | ServiceAction::Restart)
                ) {
                    self.pending_service = None;
                    self.service_deadline = None;
                }
                // Toast the recovery transition: the banner already covers
                // the loss, but its silent disappearance is easy to miss.
                let was_connected = matches!(
                    self.state.as_ref().map(|old| &old.keyboard),
                    Some(KeyboardStatus::Connected)
                );
                let now_connected = matches!(state.keyboard, KeyboardStatus::Connected);
                let recovery_was_visible = self
                    .recovery_started_at
                    .is_some_and(|started| started.elapsed() >= RECOVERY_NOTICE_DELAY);
                if now_connected && !was_connected && self.state.is_some() && recovery_was_visible {
                    let text = if self.keyboard_seen {
                        "Keyboard reconnected"
                    } else {
                        "Keyboard connected"
                    };
                    self.queue_toast(text);
                }
                if now_connected {
                    self.keyboard_seen = true;
                    self.recovery_started_at = None;
                    if (connection_restored || !was_connected)
                        && self
                            .problem
                            .as_ref()
                            .is_some_and(|problem| problem.help == Help::Connection)
                    {
                        self.problem = None;
                    }
                } else if self.recovery_started_at.is_none() {
                    self.recovery_started_at = Some(Instant::now());
                }

                // The daemon wins unless the user is mid-edit, in which
                // case a snapshot older than the edit would undo it.
                if !self.editing_locally() {
                    self.lighting = active_lighting(&state);
                    self.last_local_edit_at = None;
                }
                self.state = Some(*state);
            }
            IpcUpdate::RequestFailed { kind, message } => {
                self.problem = Some(problems::request_problem(kind, message));
            }
            IpcUpdate::Incompatible(message) => {
                // Terminal: the worker has stopped, so the window says why
                // rather than sitting on a reconnect spinner forever.
                self.connected = false;
                self.state = None;
                self.incompatible = Some(message);
            }
        }
    }

    /// Send the local lighting to the daemon, targeting whichever slot is
    /// active; the resulting StateChanged event confirms it (and updates
    /// every other connected client).
    fn push_lighting(&mut self) {
        self.last_local_edit_at = Some(Instant::now());
        self.send_request(Request::SetLighting {
            slot: None,
            lighting: self.lighting.clone(),
        });
    }

    /// True while a local edit still outranks incoming snapshots.
    fn editing_locally(&self) -> bool {
        match self.last_local_edit_at {
            Some(edited_at) => edited_at.elapsed() < LOCAL_EDIT_GRACE,
            None => false,
        }
    }

    fn queue_toast(&self, text: &str) {
        self.pending_toast.set(Some(text.to_string()));
    }

    /// Whether this machine has a battery, per the daemon. False before
    /// state arrives, which is the safe way round: the effects that need
    /// one stay out of the picker until the daemon confirms it.
    fn battery_available(&self) -> bool {
        match &self.state {
            Some(state) => state.battery_available,
            None => false,
        }
    }

    fn sync_lighting_page(&self, page: &lighting::LightingPage) {
        // Nothing on this page can be decided without the daemon's slot, so
        // the whole sync waits for state rather than syncing the editors
        // from defaults and correcting them a moment later.
        let Some(state) = &self.state else {
            return;
        };
        let active_slot = state.active_slot;

        // The off position holds no lighting: the daemon rejects an edit
        // aimed at it, so the editors are removed rather than left on screen
        // to be refused. Same rule as a setting an effect does not use, one
        // level up.
        let slot_is_lit = active_slot.index().is_some();

        // Effect combo. The list and the selection go together: the
        // selection is a position in the list, so they are never allowed to
        // be out of step, not even for one signal.
        let effect_names = lighting::effect_names(state.battery_available, self.lighting.effect);
        let selected_effect = effect_index(state.battery_available, self.lighting.effect);
        page.set_effects(&effect_names, selected_effect);

        // Zone colors.
        for (button, zone) in page.zone_buttons.iter().zip(self.lighting.rgb_zones.iter()) {
            let shown = lighting::rgba_to_bytes(&button.rgba());
            if shown != zone.rgb {
                button.set_rgba(&lighting::bytes_to_rgba(zone.rgb));
            }
        }

        // Options.
        let shown_speed = page.speed_row.value() as u8;
        if shown_speed != self.lighting.speed {
            page.speed_row.set_value(f64::from(self.lighting.speed));
        }

        let high = self.lighting.brightness == Brightness::High;
        if page.brightness_row.is_active() != high {
            page.brightness_row.set_active(high);
        }

        let direction_index: u32 = match self.lighting.direction {
            Direction::Left => 0,
            Direction::Right => 1,
        };
        if page.direction_row.selected() != direction_index {
            page.direction_row.set_selected(direction_index);
        }

        // A control the effect ignores is removed, not greyed out: a dimmed
        // Direction row under Static is a permanent invitation to wonder what
        // would happen if it were reachable.
        //
        // `get_visible`, not `is_visible`: the latter reports effective
        // visibility, so every one of these compares reads false while the
        // page sits behind the disconnected view, the write is skipped, and
        // the row's own flag stays whatever it was. It reappears wrong when
        // the page comes back.
        if page.effect_group.get_visible() != slot_is_lit {
            page.effect_group.set_visible(slot_is_lit);
        }
        if page.options_group.get_visible() != slot_is_lit {
            page.options_group.set_visible(slot_is_lit);
        }

        let takes_colors = slot_is_lit && self.lighting.effect.takes_color_array();
        if page.colors_group.get_visible() != takes_colors {
            page.colors_group.set_visible(takes_colors);
        }
        let takes_speed = self.lighting.effect.takes_speed();
        if page.speed_row.get_visible() != takes_speed {
            page.speed_row.set_visible(takes_speed);
        }
        let takes_direction = self.lighting.effect.takes_direction();
        if page.direction_row.get_visible() != takes_direction {
            page.direction_row.set_visible(takes_direction);
        }

        // Per-effect groups.
        let is_ambient =
            slot_is_lit && matches!(self.lighting.effect, Effects::AmbientLight { .. });
        if page.ambient_group.get_visible() != is_ambient {
            page.ambient_group.set_visible(is_ambient);
        }
        if let Effects::AmbientLight {
            fps,
            saturation_boost,
        } = self.lighting.effect
        {
            let shown_fps = page.fps_row.value() as u8;
            if shown_fps != fps {
                page.fps_row.set_value(f64::from(fps));
            }
            let shown_saturation = page.saturation_row.value() as f32;
            if (shown_saturation - saturation_boost).abs() > 0.001 {
                page.saturation_row.set_value(f64::from(saturation_boost));
            }
        }

        let is_swipe = slot_is_lit
            && matches!(
                self.lighting.effect,
                Effects::Swipe { .. } | Effects::SmoothWave { .. }
            );
        if page.swipe_group.get_visible() != is_swipe {
            page.swipe_group.set_visible(is_swipe);
        }
        if let Effects::Swipe {
            mode,
            clean_with_black,
        }
        | Effects::SmoothWave {
            mode,
            clean_with_black,
        } = self.lighting.effect
        {
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
            // Only the fill mode has anything to wipe.
            let clean_applies = mode == SwipeMode::Fill;
            if page.clean_row.get_visible() != clean_applies {
                page.clean_row.set_visible(clean_applies);
            }
        }

        // --- Slots ---------------------------------------------------------
        page.set_active_slot(active_slot);

        // Say so where the key would have been used, rather than letting
        // "Fn+Space cycles them" imply something untrue on this machine.
        // The battery takes this line first: it explains a keyboard that
        // is not showing the slot the page is describing, which is more
        // urgent than anything Fn+Space has to say.
        let slot_note = if state.battery_alert_active {
            Some(
                "Battery is low, so the keyboard is showing red. \
                 Your lighting returns when you plug in."
                    .to_string(),
            )
        } else {
            match &state.slot_sync {
                SubsystemState::Unavailable { reason } => Some(format!(
                    "Fn+Space is not available here ({reason}). Use these buttons instead."
                )),
                SubsystemState::Degraded { reason } => Some(format!(
                    "Fn+Space may be out of step ({reason}). Pick a slot to resync."
                )),
                _ => None,
            }
        };
        page.set_slot_note(slot_note.as_deref());

        // --- Preview and off state -------------------------------------------
        // Two states, never both. Lit: the preview card shows what the
        // keyboard is doing, with nothing zone-coloured for effects that
        // ignore zone colours, and a caption naming the slot. Off: there is
        // no lighting to preview and none to edit, so the card and its
        // caption go and the off mark takes the space. An empty card left
        // on screen reads as a rendering fault, not as "off".
        let lit_slot = active_slot.index();
        let backlight_off = lit_slot.is_none();

        if page.preview_box.get_visible() == backlight_off {
            page.preview_box.set_visible(!backlight_off);
        }
        if page.off_state.get_visible() != backlight_off {
            page.off_state.set_visible(backlight_off);
        }

        if let Some(slot_index) = lit_slot {
            // The preview draws the keyboard, so it draws the alert when
            // the alert is what the keyboard is showing. Drawing the
            // stored colours here would have the app contradict the
            // hardware in front of the user.
            let preview_colors = if state.battery_alert_active {
                [BATTERY_ALERT_PREVIEW_COLOR; 4]
            } else {
                preview_zone_colors(&self.lighting, state.battery_percent)
            };
            page.preview.set_colors(preview_colors);

            let effect = state.current.slots[slot_index].effect;
            let slot_text = format!("Slot {active_slot} of {SLOT_COUNT} \u{00b7} {effect}");
            if page.slot_label.text() != slot_text.as_str() {
                page.slot_label.set_text(&slot_text);
            }
            if !page.slot_label.get_visible() {
                page.slot_label.set_visible(true);
            }
        }
    }
}

/// Zone colours for the preview, or black for effects that do not use
/// them. Showing the stored zone colours under a rainbow effect was the
/// preview claiming something the keyboard was not doing.
///
/// The Battery effect is the same rule one step further: it uses the zone
/// colours, but dims them, so the preview dims them too.
///
/// With no charge to draw at, the preview falls through to the undimmed
/// colours, because that is what the daemon puts on the keyboard when it
/// cannot read a charge either. Drawing darkness here would be the app
/// contradicting the hardware, which is the whole reason this function
/// takes the effect into account at all.
fn preview_zone_colors(lighting: &Lighting, battery_percent: Option<u8>) -> [[u8; 3]; 4] {
    let mut colors: [[u8; 3]; 4] = [[0; 3]; 4];
    if !lighting.effect.takes_color_array() {
        return colors;
    }

    if let (true, Some(percent)) = (lighting.effect.needs_a_battery(), battery_percent) {
        let gauge = lighting.battery_gauge_array(percent);
        for (zone_index, target) in colors.iter_mut().enumerate() {
            let byte_offset = zone_index * COLOR_CHANNELS_PER_ZONE;
            *target = [
                gauge[byte_offset],
                gauge[byte_offset + 1],
                gauge[byte_offset + 2],
            ];
        }

        return colors;
    }

    for (target, zone) in colors.iter_mut().zip(lighting.rgb_zones.iter()) {
        if zone.enabled {
            *target = zone.rgb;
        }
    }

    colors
}

/// The lighting the keyboard is showing: the active slot's, or darkness
/// while the backlight is off.
fn active_lighting(state: &DaemonState) -> Lighting {
    match state.active_slot.index() {
        Some(slot_index) => state.current.slots[slot_index].clone(),
        None => Lighting::default(),
    }
}

/// The effect at a combo row position, with usable defaults for the
/// field-carrying variants (the picker's list yields zeroed fields).
///
/// `active` is what the daemon reports, because the list the user is
/// looking at includes it even when this machine cannot start it.
fn effect_by_index(battery_available: bool, active: Effects, index: usize) -> Option<Effects> {
    let effect = lighting::selectable_effects(battery_available, active)
        .into_iter()
        .nth(index)?;

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

fn effect_index(battery_available: bool, effect: Effects) -> Option<usize> {
    for (index, candidate) in lighting::selectable_effects(battery_available, effect)
        .into_iter()
        .enumerate()
    {
        if candidate.same_variant(effect) {
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

    let dialog = adw::AlertDialog::new(
        Some("Save Profile"),
        Some("Save the current lighting as a profile."),
    );
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("save", "Save");
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("save", false);
    let dialog_for_entry = dialog.downgrade();
    entry.connect_changed(move |entry| {
        if let Some(dialog) = dialog_for_entry.upgrade() {
            dialog.set_response_enabled("save", !entry.text().trim().is_empty());
        }
    });

    // Enter in the entry saves, matching the suggested response; without
    // this the key press is swallowed by the entry.
    let activate_sender = sender.clone();
    let dialog_for_activate = dialog.downgrade();
    entry.connect_activate(move |entry| {
        let name = entry.text().trim().to_string();
        if name.is_empty() {
            return;
        }
        activate_sender.input(AppMsg::SaveProfileConfirmed { name });
        if let Some(dialog) = dialog_for_activate.upgrade() {
            dialog.close();
        }
    });

    let dialog_sender = sender.clone();
    dialog.connect_response(None, move |dialog, response| {
        if response == "save" {
            let name = dialog
                .extra_child()
                .and_downcast::<gtk::Entry>()
                .map(|entry| entry.text().trim().to_string())
                .unwrap_or_default();
            dialog_sender.input(AppMsg::SaveProfileConfirmed { name });
        }
    });

    dialog.present(Some(&window));
}

fn show_about_dialog() {
    let Some(window) = relm4::main_application().active_window() else {
        return;
    };

    let dialog = adw::AboutDialog::new();
    dialog.set_application_name("Aurora");
    dialog.set_application_icon("io.github.HughScott2002.Aurora");
    dialog.set_version(env!("CARGO_PKG_VERSION"));
    dialog.set_developer_name("Hugh Scott");
    dialog.set_license_type(gtk::License::Gpl30);
    dialog.set_website(links::REPOSITORY_URL);
    dialog.set_issue_url(links::NEW_ISSUE_URL);
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

    let dialog = gtk::FileDialog::builder()
        .title("Load Custom Effect")
        .filters(&filters)
        .build();

    let dialog_sender = sender.clone();
    dialog.open(
        Some(&window),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            let file = match result {
                Ok(file) => file,
                Err(_) => return, // Dismissed.
            };
            if let Some(path) = file.path() {
                dialog_sender.input(AppMsg::CustomEffectFileChosen { path });
            }
        },
    );
}

fn read_custom_effect_file(
    path: &std::path::Path,
) -> Result<aurora_protocol::custom_effect::CustomEffect, String> {
    use std::io::Read;

    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_EFFECT_FILE_BYTES {
        return Err("Choose a regular Aurora effect file smaller than one mebibyte.".to_string());
    }
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut contents = String::new();
    file.take(MAX_EFFECT_FILE_BYTES + 1)
        .read_to_string(&mut contents)
        .map_err(|error| error.to_string())?;
    if contents.len() as u64 > MAX_EFFECT_FILE_BYTES {
        return Err("The effect file is too large.".to_string());
    }
    let mut effect: aurora_protocol::custom_effect::CustomEffect =
        serde_json::from_str(&contents).map_err(|error| error.to_string())?;
    if effect.name.is_none() {
        effect.name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned());
    }
    Ok(effect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_and_oversized_effect_files_are_rejected() {
        let path = std::env::temp_dir().join(format!("aurora-invalid-{}.json", std::process::id()));
        std::fs::write(&path, "not an effect").expect("write fixture");
        assert!(read_custom_effect_file(&path).is_err());
        let file = std::fs::File::create(&path).expect("create fixture");
        file.set_len(MAX_EFFECT_FILE_BYTES + 1)
            .expect("size fixture");
        assert!(read_custom_effect_file(&path).is_err());
        std::fs::remove_file(path).expect("remove fixture");
        assert!(read_custom_effect_file(&std::env::temp_dir()).is_err());
    }

    #[test]
    fn a_valid_unnamed_effect_uses_the_file_name() {
        let stem = format!("aurora-effect-{}", std::process::id());
        let path = std::env::temp_dir().join(format!("{stem}.json"));
        let contents = r#"{
            "name": null,
            "effect_steps": [{
                "rgb_array": [0,0,0,0,0,0,0,0,0,0,0,0],
                "step_type": "Set", "brightness": 1, "steps": 1,
                "delay_between_steps": 0, "sleep": 10
            }],
            "should_loop": false
        }"#;
        std::fs::write(&path, contents).expect("write fixture");
        let effect = read_custom_effect_file(&path).expect("read fixture");
        assert_eq!(effect.name.as_deref(), Some(stem.as_str()));
        assert_eq!(effect.effect_steps.len(), 1);
        std::fs::remove_file(path).expect("remove fixture");
    }
}
