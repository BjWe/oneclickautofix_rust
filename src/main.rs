#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod command;
mod config;
mod embed;
mod executor;

use eframe::egui;
use app::{ErrorApp, OneClickApp};

fn main() {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("one click auto fix")
            .with_inner_size([640.0, 520.0])
            .with_min_inner_size([420.0, 300.0]),
        ..Default::default()
    };

    let app: Box<dyn eframe::App> = match load() {
        Ok(a)  => Box::new(a),
        Err(e) => Box::new(ErrorApp { message: e }),
    };

    eframe::run_native("one click auto fix", native_options, Box::new(|_cc| Ok(app)))
        .unwrap_or_else(|e| eprintln!("eframe Fehler: {e}"));
}

fn load() -> Result<OneClickApp, String> {
    let json = embed::read_embedded_config().ok_or_else(|| {
        "Keine Konfiguration in der EXE eingebettet.\n\
         Bitte die EXE mit dem config_embed-Tool versehen:\n\
         config_embed attach oneclickautofix.exe config.json"
            .to_string()
    })?;
    let cfg = config::parse_config(&json)?;
    Ok(OneClickApp::new(cfg))
}
