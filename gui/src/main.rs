mod app;
mod daemon_actions;
mod ipc;
mod pages;
mod preview;

use relm4::RelmApp;

const APP_ID: &str = "io.github.HughScott2002.Aurora";

fn main() {
    let app = RelmApp::new(APP_ID);
    app.run::<app::App>(());
}
