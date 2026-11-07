//! Tela de senha exibida antes de abrir o sistema: criação da senha na primeira vez e
//! desbloqueio nas seguintes.

use super::dialogs::field;
use super::theme::{self, Btn};
use crate::crypto::Vault;
use crate::db::Db;
use crate::security::{self, SecurityError};
use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Primeira vez: criar a senha (ou seguir sem ela).
    Setup,
    Unlock,
}

pub struct LockScreen {
    db: Option<Db>,
    mode: Mode,
    password: String,
    confirm: String,
    error: Option<String>,
}

impl LockScreen {
    pub fn new(db: Db) -> Self {
        let mode = if security::is_configured(&db) { Mode::Unlock } else { Mode::Setup };
        Self { db: Some(db), mode, password: String::new(), confirm: String::new(), error: None }
    }

    /// Desenha a tela. Devolve o banco e a chave quando o usuário conseguiu entrar.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<(Db, Option<Vault>)> {
        let mut outcome = None;
        egui::CentralPanel::default().frame(theme::content_frame()).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.18).max(16.0));
                ui.allocate_ui(egui::vec2(460.0, 0.0), |ui| {
                    theme::card(ui, |ui| outcome = self.card(ui));
                });
            });
        });
        outcome
    }

    fn card(&mut self, ui: &mut egui::Ui) -> Option<(Db, Option<Vault>)> {
        let mut submit = false;
        let mut skip = false;
        ui.label(theme::title(format!("{}  {}", icon::LOCK_KEY, crate::ui::APP_NAME)));
        ui.add_space(6.0);
        match self.mode {
            Mode::Unlock => {
                ui.label(theme::dim("Digite a senha para abrir o sistema."));
                ui.add_space(8.0);
                let response = field(ui, "Senha", &mut self.password, "", true);
                response.request_focus();
                submit |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(4.0);
                if let Some(e) = &self.error {
                    ui.label(RichText::new(format!("{}  {e}", icon::WARNING)).color(theme::DANGER));
                }
                if ui.add(Btn::primary(format!("{}  Entrar", icon::SIGN_IN))).clicked() {
                    submit = true;
                }
            }
            Mode::Setup => {
                ui.label(theme::dim(
                    "Crie uma senha para proteger os dados. Ela criptografa os tokens de pagamento e \
                     os backups enviados ao Google Drive, e é pedida toda vez que o sistema abre.",
                ));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "{}  Não há como recuperar a senha. Se for esquecida, os tokens precisam ser \
                         cadastrados de novo e os backups criptografados não poderão mais ser abertos.",
                        icon::WARNING
                    ))
                    .color(theme::WARNING),
                );
                ui.add_space(8.0);
                field(ui, "Nova senha", &mut self.password, &format!("pelo menos {} caracteres", security::MIN_PASSWORD_LEN), true);
                let response = field(ui, "Repita a senha", &mut self.confirm, "", true);
                submit |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if let Some(e) = &self.error {
                    ui.label(RichText::new(format!("{}  {e}", icon::WARNING)).color(theme::DANGER));
                }
                ui.horizontal(|ui| {
                    if ui.add(Btn::primary(format!("{}  Criar senha", icon::CHECK))).clicked() {
                        submit = true;
                    }
                    if ui.add(Btn::secondary("Continuar sem senha")).clicked() {
                        skip = true;
                    }
                });
                ui.label(theme::faint("Sem senha, os tokens ficam em texto puro e não é possível usar o Google Drive."));
            }
        }

        let db = self.db.as_ref()?;
        if skip {
            return match security::skip(db) {
                Ok(()) => self.db.take().map(|db| (db, None)),
                Err(e) => {
                    self.error = Some(e.to_string());
                    None
                }
            };
        }
        if !submit {
            return None;
        }
        let result = match self.mode {
            Mode::Unlock => security::unlock(db, &self.password),
            Mode::Setup if self.password != self.confirm => {
                self.error = Some("As senhas não são iguais.".into());
                return None;
            }
            Mode::Setup => security::create(db, &self.password),
        };
        match result {
            Ok(vault) => {
                self.password.clear();
                self.confirm.clear();
                self.db.take().map(|db| (db, Some(vault)))
            }
            Err(SecurityError::WrongPassword) => {
                self.password.clear();
                self.error = Some("Senha incorreta.".into());
                None
            }
            Err(e) => {
                self.error = Some(e.to_string());
                None
            }
        }
    }
}
