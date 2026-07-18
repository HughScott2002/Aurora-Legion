//! CLI subcommands. Each one is a thin client of the daemon socket; `set`
//! and `load-profile` fall back to driving the keyboard directly when no
//! daemon is running (hardware effects only — software effects need a
//! process that stays alive, which is the daemon's job).

use std::{convert::TryInto, path::PathBuf, process::ExitCode, str::FromStr};

use clap::{Args, Subcommand};
use aurora_protocol::{
    custom_effect::CustomEffect,
    effects::{Brightness, Direction, Effects},
    ipc::{Request, Response},
    profile::{arr_to_zones, Profile},
};
use strum::IntoEnumIterator;

use crate::{
    client::{Client, ClientError},
    engine::{EffectManager, StopSignals},
    keyboard::{self, AcquireOutcome},
};

#[derive(Subcommand)]
pub enum ClientCommand {
    /// Apply an effect from the built-in set
    Set(SetArgs),

    /// List all the available effects
    List,

    /// Show daemon and keyboard status
    Status,

    /// Switch to the next saved profile
    CycleProfile,

    /// Load and apply a profile from a file
    LoadProfile {
        #[arg(short, long)]
        path: PathBuf,
    },

    /// Load and play a custom effect from a file
    CustomEffect {
        #[arg(short, long)]
        path: PathBuf,
    },

    /// Stop the playing custom effect and restore the current profile
    Stop,

    /// Ask a running daemon to exit
    Shutdown,
}

#[derive(Args)]
pub struct SetArgs {
    /// The effect to be set
    #[arg(short, long, value_parser = parse_effect)]
    effect: Effects,

    /// List of 4 RGB triplets. Example: 255,0,0,255,255,0,0,0,255,255,128,0
    #[arg(short, long, value_parser = parse_colors)]
    colors: Option<[u8; 12]>,

    /// The brightness of the effect [possible values: Low, High]
    #[arg(short, long, default_value = "Low", value_parser = parse_brightness)]
    brightness: Brightness,

    /// The speed of the effect (1-4 for hardware effects, 1-10 for software ones)
    #[arg(short, long, default_value_t = 1)]
    speed: u8,

    /// The direction of the effect (if applicable) [possible values: Left, Right]
    #[arg(short, long, value_parser = parse_direction)]
    direction: Option<Direction>,

    /// A filename to save the profile at
    #[arg(long)]
    save: Option<PathBuf>,
}

fn parse_effect(arg: &str) -> Result<Effects, String> {
    match Effects::from_str(arg) {
        Ok(effect) => Ok(effect),
        Err(_) => {
            let mut names: Vec<&'static str> = Vec::new();
            for effect in Effects::iter() {
                names.push(effect.into());
            }
            Err(format!("unknown effect '{arg}', expected one of: {}", names.join(", ")))
        }
    }
}

fn parse_brightness(arg: &str) -> Result<Brightness, String> {
    Brightness::from_str(arg).map_err(|_| format!("unknown brightness '{arg}', expected Low or High"))
}

fn parse_direction(arg: &str) -> Result<Direction, String> {
    Direction::from_str(arg).map_err(|_| format!("unknown direction '{arg}', expected Left or Right"))
}

fn parse_colors(arg: &str) -> Result<[u8; 12], String> {
    let mut parsed: Vec<u8> = Vec::with_capacity(12);
    for part in arg.split(',') {
        let value = part.trim().parse::<u8>().map_err(|_| format!("'{part}' is not a number between 0 and 255"))?;
        parsed.push(value);
    }

    let count = parsed.len();
    let colors: [u8; 12] = parsed.try_into().map_err(|_| format!("expected 12 comma-separated values (4 RGB triplets), got {count}"))?;
    Ok(colors)
}

pub fn run(command: ClientCommand) -> ExitCode {
    match command {
        ClientCommand::List => {
            println!("List of available effects:");
            for (index, effect) in Effects::iter().enumerate() {
                println!("{}. {effect}", index + 1);
            }
            ExitCode::SUCCESS
        }
        ClientCommand::Status => run_status(),
        ClientCommand::Set(args) => run_set(&args),
        ClientCommand::CycleProfile => run_simple_request(Request::CycleProfile, "profile cycled"),
        ClientCommand::LoadProfile { path } => run_load_profile(&path),
        ClientCommand::CustomEffect { path } => run_custom_effect(&path),
        ClientCommand::Stop => run_simple_request(Request::StopCustomEffect, "custom effect stopped"),
        ClientCommand::Shutdown => run_simple_request(Request::Shutdown, "daemon asked to exit"),
    }
}

fn run_status() -> ExitCode {
    let mut client = match Client::connect() {
        Ok(client) => client,
        Err(_) => {
            println!("daemon:   not running");
            println!("start it with: aurora daemon   (or systemctl --user start aurora)");
            return ExitCode::FAILURE;
        }
    };

    let response = match client.request(Request::GetState) {
        Ok(response) => response,
        Err(error) => {
            eprintln!("aurora: {error}");
            return ExitCode::FAILURE;
        }
    };

    let Response::State { state } = response else {
        eprintln!("aurora: unexpected response to GetState");
        return ExitCode::FAILURE;
    };

    println!("daemon:   running (v{})", state.version);
    match state.keyboard {
        aurora_protocol::ipc::KeyboardStatus::Connected => println!("keyboard: connected"),
        aurora_protocol::ipc::KeyboardStatus::Searching => println!("keyboard: searching..."),
        aurora_protocol::ipc::KeyboardStatus::PermissionDenied { message } => {
            println!("keyboard: permission denied ({message})");
            println!("          install the udev rule: https://github.com/4JX/L5P-Keyboard-RGB#usage");
        }
        aurora_protocol::ipc::KeyboardStatus::Error { message } => println!("keyboard: error ({message})"),
    }

    let profile_name = state.current.name.unwrap_or_else(|| "(unsaved)".to_string());
    println!("profile:  {profile_name} — {} effect", state.current.effect);
    if let Some(name) = state.custom_effect_playing {
        println!("playing:  custom effect '{name}'");
    }

    let mut saved_names: Vec<String> = Vec::new();
    for profile in &state.profiles {
        if let Some(name) = &profile.name {
            saved_names.push(name.clone());
        }
    }
    println!("saved:    {} profiles ({})", state.profiles.len(), saved_names.join(", "));

    ExitCode::SUCCESS
}

fn run_set(args: &SetArgs) -> ExitCode {
    let rgb_array = if args.effect.takes_color_array() {
        match args.colors {
            Some(colors) => colors,
            None => {
                eprintln!("aurora: the {} effect requires --colors", args.effect);
                return ExitCode::FAILURE;
            }
        }
    } else {
        [0; 12]
    };

    let mut profile = Profile {
        name: None,
        rgb_zones: arr_to_zones(rgb_array),
        effect: args.effect,
        direction: args.direction.unwrap_or_default(),
        speed: args.speed,
        brightness: args.brightness,
    };

    if let Some(save_path) = &args.save {
        let save_result = profile.save_profile(save_path);
        if save_result.is_err() {
            eprintln!("aurora: could not save profile to {}", save_path.display());
            return ExitCode::FAILURE;
        }
        println!("profile saved to {}", save_path.display());
    }

    apply_profile(profile)
}

fn run_load_profile(path: &PathBuf) -> ExitCode {
    let profile = match Profile::load_profile(path) {
        Ok(profile) => profile,
        Err(_) => {
            eprintln!("aurora: could not load profile from {}", path.display());
            return ExitCode::FAILURE;
        }
    };

    apply_profile(profile)
}

fn apply_profile(profile: Profile) -> ExitCode {
    match Client::connect() {
        Ok(mut client) => {
            let response = client.request(Request::SetProfile { profile });
            print_outcome(response, "profile applied")
        }
        Err(_) => apply_profile_directly(&profile),
    }
}

fn apply_profile_directly(profile: &Profile) -> ExitCode {
    if !profile.effect.is_built_in() {
        eprintln!("aurora: no daemon is running, and the {} effect needs one (it is software-driven).", profile.effect);
        eprintln!("           start the daemon with: aurora daemon");
        return ExitCode::FAILURE;
    }

    println!("no daemon running, applying directly to the keyboard");

    let stop_signals = StopSignals::new();
    let keyboard = match keyboard::try_acquire(&stop_signals) {
        AcquireOutcome::Acquired(keyboard) => keyboard,
        AcquireOutcome::Failed(status) => {
            eprintln!("aurora: could not open the keyboard: {status:?}");
            return ExitCode::FAILURE;
        }
    };

    let engine = EffectManager::new(*keyboard, stop_signals);
    engine.set_profile(profile.clone());
    engine.shutdown();

    // Hardware effects persist after we exit; nothing else to do.
    ExitCode::SUCCESS
}

fn run_custom_effect(path: &PathBuf) -> ExitCode {
    let effect = match CustomEffect::from_file(path) {
        Ok(effect) => effect,
        Err(_) => {
            eprintln!("aurora: could not load custom effect from {}", path.display());
            return ExitCode::FAILURE;
        }
    };

    match Client::connect() {
        Ok(mut client) => {
            let response = client.request(Request::PlayCustomEffect { effect });
            print_outcome(response, "custom effect playing")
        }
        Err(_) => {
            eprintln!("aurora: no daemon is running; custom effects need one.");
            eprintln!("           start the daemon with: aurora daemon");
            ExitCode::FAILURE
        }
    }
}

fn run_simple_request(request: Request, success_message: &str) -> ExitCode {
    let mut client = match Client::connect() {
        Ok(client) => client,
        Err(_) => {
            eprintln!("aurora: no daemon is running.");
            return ExitCode::FAILURE;
        }
    };

    let response = client.request(request);
    print_outcome(response, success_message)
}

fn print_outcome(response: Result<Response, ClientError>, success_message: &str) -> ExitCode {
    match response {
        Ok(Response::Ok) => {
            println!("{success_message}");
            ExitCode::SUCCESS
        }
        Ok(Response::Error { kind, message }) => {
            eprintln!("aurora: {kind:?}: {message}");
            ExitCode::FAILURE
        }
        Ok(_) => {
            eprintln!("aurora: unexpected response type");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("aurora: {error}");
            ExitCode::FAILURE
        }
    }
}
