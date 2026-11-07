#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;
mod billing;
mod db;
mod money;
mod services;
mod ui;

use std::path::PathBuf;

fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("CaderninhoFiado");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = data_dir();
    let db = db::Db::open(&dir.join("fiado.db"))?;
    let app = app::App::new(db, &dir)?;
    let options = eframe::NativeOptions {
        // Sempre maximizada: ocupa a tela toda, mas mantém a barra de tarefas visível.
        viewport: eframe::egui::ViewportBuilder::default()
            .with_maximized(true)
            .with_min_inner_size([900.0, 600.0])
            .with_title("Gestor de Crédito"),
        ..Default::default()
    };
    eframe::run_native(
        "Gestor de Crédito",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
