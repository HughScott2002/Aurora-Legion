mod app;
mod daemon_actions;
mod ipc;
mod pages;
mod preview;

use relm4::RelmApp;

const APP_ID: &str = "com.github.hugh.LegionKbRgb";

fn main() {
    let app = RelmApp::new(APP_ID);
    app.run::<app::App>(());
}
