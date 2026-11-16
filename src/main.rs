#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use forgedock::ui::{self, Options};
use std::path::PathBuf;

const USAGE: &str = "ForgeDock

  ForgeDock.exe                      open the app manager
  ForgeDock.exe --demo               show built-in data; no network, nothing on this PC is changed
  ForgeDock.exe --screenshot <png>   render the window to a PNG and exit
        [--view discover|app|app:<id>|gallery|installed|updates|settings|about] [--light]
        [--live-images]              with --demo: still load screenshots from GitHub";

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if has("--help") || has("-h") || has("/?") {
        println!("{USAGE}");
        return Ok(());
    }
    let options = Options {
        demo: has("--demo"),
        screenshot: value("--screenshot").map(PathBuf::from),
        view: value("--view"),
        light: has("--light"),
        live_images: has("--live-images"),
    };
    ui::run(options, window_icon())
}

fn window_icon() -> egui::IconData {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/icon-256.png"));
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let rgba = img.into_rgba8();
            let (width, height) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            }
        }
        Err(_) => egui::IconData::default(),
    }
}
