#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;
mod backup;
mod billing;
mod crypto;
mod db;
mod money;
mod security;
mod services;
mod ui;

use std::path::{Path, PathBuf};

/// Pasta dos dados do usuário. Antes se chamava `CaderninhoFiado`; se ainda existir e a nova
/// não, os dados são movidos. Se não der para mover (arquivos em uso), continua na antiga.
fn resolve_data_dir(base: &Path) -> PathBuf {
    let new = base.join("GestorDeCredito");
    let old = base.join("CaderninhoFiado");
    if !new.exists() && old.exists() && std::fs::rename(&old, &new).is_err() {
        return old;
    }
    let _ = std::fs::create_dir_all(&new);
    new
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    resolve_data_dir(&base)
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
            .with_title(ui::APP_NAME),
        ..Default::default()
    };
    eframe::run_native(
        ui::APP_NAME,
        options,
        Box::new(move |cc| {
            let mut fonts = eframe::egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            cc.egui_ctx.set_fonts(fonts);
            ui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_base(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("credit-manager-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn old_data_folder_is_migrated() {
        let base = temp_base("migrate");
        std::fs::create_dir_all(base.join("CaderninhoFiado")).unwrap();
        std::fs::write(base.join("CaderninhoFiado").join("fiado.db"), b"dados").unwrap();

        let dir = resolve_data_dir(&base);
        assert_eq!(dir, base.join("GestorDeCredito"));
        assert_eq!(std::fs::read(dir.join("fiado.db")).unwrap(), b"dados");
        assert!(!base.join("CaderninhoFiado").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn new_folder_wins_and_nothing_is_overwritten() {
        let base = temp_base("keep-new");
        std::fs::create_dir_all(base.join("CaderninhoFiado")).unwrap();
        std::fs::create_dir_all(base.join("GestorDeCredito")).unwrap();
        std::fs::write(base.join("GestorDeCredito").join("fiado.db"), b"novo").unwrap();

        let dir = resolve_data_dir(&base);
        assert_eq!(std::fs::read(dir.join("fiado.db")).unwrap(), b"novo");
        assert!(base.join("CaderninhoFiado").exists(), "a pasta antiga não deve ser apagada");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn fresh_install_creates_the_new_folder() {
        let base = temp_base("fresh");
        let dir = resolve_data_dir(&base);
        assert!(dir.is_dir());
        assert_eq!(dir.file_name().unwrap(), "GestorDeCredito");
        let _ = std::fs::remove_dir_all(base);
    }
}
