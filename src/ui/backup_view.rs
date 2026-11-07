use super::View;
use super::dialogs::{DialogAction, Outcome, field, form_dialog, secret_field};
use super::theme::{self, Btn, Tone};
use crate::backup::{self, LocalBackup};
use crate::security;
use crate::services::Services;
use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;

#[derive(Default)]
pub struct BackupView {
    local: Vec<LocalBackup>,
    local_revision: Option<u64>,
    password_form: Option<PasswordForm>,
    drive_form: Option<DriveForm>,
    file_form: Option<FileForm>,
    restore: Option<RestoreForm>,
}

// ------------------------------------------------------------ senha

struct PasswordForm {
    change: bool,
    old: String,
    new: String,
    confirm: String,
    error: Option<String>,
}

impl PasswordForm {
    fn new(change: bool) -> Self {
        Self { change, old: String::new(), new: String::new(), confirm: String::new(), error: None }
    }

    fn show(&mut self, ctx: &egui::Context, sv: &mut Services) -> Outcome {
        let title = if self.change { "Trocar a senha" } else { "Criar a senha" };
        let action = form_dialog(
            ctx,
            "password_form",
            &format!("{}  {title}", icon::LOCK_KEY),
            "Salvar",
            self.error.as_deref(),
            |ui| {
                if self.change {
                    field(ui, "Senha atual", &mut self.old, "", true);
                }
                field(ui, "Nova senha", &mut self.new, &format!("pelo menos {} caracteres", security::MIN_PASSWORD_LEN), true);
                field(ui, "Repita a nova senha", &mut self.confirm, "", true);
                ui.label(
                    RichText::new(format!(
                        "{}  Não há como recuperar a senha. {}",
                        icon::WARNING,
                        if self.change {
                            "Backups feitos antes da troca continuam exigindo a senha antiga."
                        } else {
                            "Sem ela, os backups criptografados não podem ser abertos."
                        }
                    ))
                    .color(theme::WARNING),
                );
            },
        );
        match action {
            DialogAction::None => Outcome::Open,
            DialogAction::Cancel => Outcome::Cancelled,
            DialogAction::Submit => {
                if self.new != self.confirm {
                    self.error = Some("As senhas não são iguais.".into());
                    return Outcome::Open;
                }
                let result = if self.change {
                    security::change_password(&sv.db, &self.old, &self.new)
                } else {
                    security::create(&sv.db, &self.new)
                };
                match result {
                    Ok(vault) => {
                        sv.vault = Some(vault);
                        sv.reload_provider();
                        sv.load_drive();
                        sv.notify(if self.change { "Senha trocada." } else { "Senha criada." });
                        Outcome::Saved
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        Outcome::Open
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------ Google Drive

struct DriveForm {
    client_id: String,
    client_secret: String,
    reveal: bool,
    error: Option<String>,
}

impl DriveForm {
    fn load(sv: &Services) -> Self {
        Self {
            client_id: sv.db.get_setting("gdrive_client_id").ok().flatten().unwrap_or_default(),
            client_secret: sv.secret("gdrive_client_secret"),
            reveal: false,
            error: None,
        }
    }

    fn show(&mut self, ctx: &egui::Context, sv: &mut Services) -> Outcome {
        let action = form_dialog(
            ctx,
            "drive_form",
            &format!("{}  Conectar ao Google Drive", icon::CLOUD),
            "Conectar",
            self.error.as_deref(),
            |ui| {
                ui.label(
                    "No Google Cloud Console, crie um projeto, ative a API do Google Drive e crie uma credencial \
                     OAuth do tipo \"Aplicativo para computador\". Cole abaixo o ID e a chave secreta do cliente. \
                     Ao conectar, o navegador abre para você autorizar.",
                );
                ui.add_space(6.0);
                field(ui, "ID do cliente", &mut self.client_id, "123456-abc.apps.googleusercontent.com", false);
                secret_field(ui, "Chave secreta do cliente", &mut self.client_secret, "GOCSPX-...", &mut self.reveal);
            },
        );
        match action {
            DialogAction::None => Outcome::Open,
            DialogAction::Cancel => Outcome::Cancelled,
            DialogAction::Submit => {
                if sv.vault.is_none() {
                    self.error = Some("Crie a senha do sistema antes: os backups enviados à nuvem são criptografados.".into());
                    return Outcome::Open;
                }
                let (id, secret) = (self.client_id.trim().to_string(), self.client_secret.trim().to_string());
                if id.is_empty() || secret.is_empty() {
                    self.error = Some("Informe o ID e a chave secreta do cliente.".into());
                    return Outcome::Open;
                }
                sv.notify("Autorize o acesso no navegador que foi aberto...");
                sv.drive_login(id, secret);
                Outcome::Saved
            }
        }
    }
}

// ------------------------------------------------------------ restauração

/// Pede o caminho de um arquivo de backup.
struct FileForm {
    path: String,
    error: Option<String>,
}

impl FileForm {
    fn show(&mut self, ctx: &egui::Context) -> Result<Option<(String, Vec<u8>)>, ()> {
        let action = form_dialog(
            ctx,
            "file_form",
            &format!("{}  Restaurar de um arquivo", icon::FOLDER_OPEN),
            "Abrir",
            self.error.as_deref(),
            |ui| {
                ui.label("Cole o caminho completo do arquivo de backup (.db ou .db.enc).");
                ui.add_space(6.0);
                field(ui, "Caminho do arquivo", &mut self.path, r"C:\Users\...\fiado-20260101-120000.db", false);
            },
        );
        match action {
            DialogAction::None => Ok(None),
            DialogAction::Cancel => Err(()),
            DialogAction::Submit => {
                let path = self.path.trim().trim_matches('"').to_string();
                match std::fs::read(&path) {
                    Ok(data) => Ok(Some((path, data))),
                    Err(e) => {
                        self.error = Some(format!("Não foi possível ler o arquivo: {e}"));
                        Ok(None)
                    }
                }
            }
        }
    }
}

struct RestoreForm {
    source: String,
    data: Vec<u8>,
    needs_password: bool,
    password: String,
    error: Option<String>,
}

impl RestoreForm {
    fn new(source: String, data: Vec<u8>, sv: &Services) -> Self {
        let salt = backup::current_salt(&sv.db);
        let needs_password = backup::needs_password(&data, salt.as_deref(), sv.vault.is_some());
        Self { source, data, needs_password, password: String::new(), error: None }
    }

    fn show(&mut self, ctx: &egui::Context, sv: &mut Services) -> Outcome {
        let action = form_dialog(
            ctx,
            "restore_form",
            &format!("{}  Restaurar backup", icon::ARROW_COUNTER_CLOCKWISE),
            "Restaurar",
            self.error.as_deref(),
            |ui| {
                ui.label(format!("Origem: {}", self.source));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "{}  Isto substitui todos os clientes, dívidas e pagamentos atuais pelos do backup. \
                         Antes, o estado atual é guardado na pasta de backups, para dar para desfazer.",
                        icon::WARNING
                    ))
                    .color(theme::WARNING),
                );
                if self.needs_password {
                    ui.add_space(6.0);
                    ui.label("Este backup é criptografado. Informe a senha que estava em uso quando ele foi feito.");
                    field(ui, "Senha do backup", &mut self.password, "", true);
                }
            },
        );
        match action {
            DialogAction::None => Outcome::Open,
            DialogAction::Cancel => Outcome::Cancelled,
            DialogAction::Submit => {
                let password = self.needs_password.then_some(self.password.as_str());
                match sv.restore_backup(self.data.clone(), password) {
                    Ok(()) => {
                        sv.notify("Backup restaurado.");
                        Outcome::Saved
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        Outcome::Open
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------ tela

fn format_time(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string())
        .unwrap_or_default()
}

fn format_system_time(t: std::time::SystemTime) -> String {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| format_time(d.as_secs() as i64)).unwrap_or_default()
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", (bytes / 1024).max(1))
    }
}

impl View for BackupView {
    fn ui(&mut self, ui: &mut egui::Ui, sv: &mut Services) {
        if self.local_revision != Some(sv.revision) {
            self.local = backup::list_local(&sv.data_dir);
            self.local_revision = Some(sv.revision);
        }
        // Um backup baixado do Drive vira o diálogo de confirmação.
        if let Some(pending) = sv.pending_restore.take() {
            self.restore = Some(RestoreForm::new(pending.source, pending.data, sv));
        }

        ui.label(theme::title(format!("{}  Backup e segurança", icon::SHIELD_CHECK)));
        ui.add_space(8.0);

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            // ---------------- segurança ----------------
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}  Senha do sistema", icon::LOCK_KEY)).heading());
                    if sv.vault.is_some() {
                        theme::badge(ui, "Protegido por senha", Tone::Success);
                    } else {
                        theme::badge(ui, "Sem senha", Tone::Warning);
                    }
                });
                ui.label(theme::dim(
                    "A senha é pedida ao abrir o sistema e criptografa os tokens e os backups enviados à nuvem. \
                     O arquivo do banco de dados em si não é criptografado.",
                ));
                if sv.vault.is_some() {
                    if ui.add(Btn::secondary(format!("{}  Trocar senha", icon::KEY))).clicked() {
                        self.password_form = Some(PasswordForm::new(true));
                    }
                } else if ui.add(Btn::primary(format!("{}  Criar senha", icon::LOCK_KEY))).clicked() {
                    self.password_form = Some(PasswordForm::new(false));
                }
            });
            ui.add_space(10.0);

            // ---------------- backup local ----------------
            theme::card(ui, |ui| {
                ui.label(RichText::new(format!("{}  Backup local", icon::DATABASE)).heading());
                ui.label(theme::dim(format!(
                    "Cópias do banco (sem tokens nem senha) em {}. São guardadas as {} mais recentes.",
                    backup::backups_dir(&sv.data_dir).display(),
                    backup::LOCAL_KEEP
                )));
                ui.horizontal(|ui| {
                    if ui.add(Btn::primary(format!("{}  Fazer backup agora", icon::FLOPPY_DISK))).clicked() {
                        sv.run_backup();
                    }
                    if ui.add(Btn::secondary(format!("{}  Restaurar de um arquivo", icon::FOLDER_OPEN))).clicked() {
                        self.file_form = Some(FileForm { path: String::new(), error: None });
                    }
                });
                let mut auto = sv.auto_backup_enabled();
                if theme::checkbox(ui, &mut auto, "Fazer backup automático a cada 24 horas").changed() {
                    let _ = sv.db.set_setting("backup_auto", if auto { "1" } else { "0" });
                }
                match sv.last_backup_at() {
                    Some(ts) => ui.label(theme::faint(format!("Último backup: {}", format_time(ts)))),
                    None => ui.label(theme::faint("Nenhum backup feito ainda.")),
                };
                ui.add_space(4.0);
                for b in &self.local {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}  {}", icon::DATABASE, b.name));
                        ui.label(theme::faint(format!("{}  {}", format_system_time(b.modified), format_size(b.size))));
                        if ui.add(Btn::secondary(format!("{}  Restaurar", icon::ARROW_COUNTER_CLOCKWISE)).compact()).clicked() {
                            match std::fs::read(&b.path) {
                                Ok(data) => self.restore = Some(RestoreForm::new(b.name.clone(), data, sv)),
                                Err(e) => sv.notify(format!("Não foi possível ler o backup: {e}")),
                            }
                        }
                    });
                }
            });
            ui.add_space(10.0);

            // ---------------- Google Drive ----------------
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}  Google Drive", icon::CLOUD)).heading());
                    if sv.drive.is_some() {
                        theme::badge(ui, "Conectado", Tone::Success);
                    } else {
                        theme::badge(ui, "Não conectado", Tone::Neutral);
                    }
                    if sv.drive_busy {
                        theme::badge(ui, "Trabalhando...", Tone::Warning);
                    }
                });
                ui.label(theme::dim(
                    "Envia uma cópia criptografada para uma pasta do seu Google Drive. O sistema só enxerga os \
                     arquivos que ele mesmo criou. São mantidas as 30 mais recentes.",
                ));
                if sv.vault.is_none() {
                    ui.label(
                        RichText::new(format!("{}  Crie a senha do sistema para usar o Google Drive.", icon::WARNING))
                            .color(theme::WARNING),
                    );
                }
                ui.horizontal_wrapped(|ui| {
                    let label = if sv.drive.is_some() { "Reconectar" } else { "Configurar e conectar" };
                    if ui.add(Btn::primary(format!("{}  {label}", icon::CLOUD))).clicked() {
                        self.drive_form = Some(DriveForm::load(sv));
                    }
                    if sv.drive.is_some() {
                        let idle = !sv.drive_busy;
                        if ui.add_enabled(idle, Btn::secondary(format!("{}  Enviar backup agora", icon::CLOUD_ARROW_UP))).clicked() {
                            sv.drive_upload_now();
                        }
                        if ui.add_enabled(idle, Btn::secondary(format!("{}  Atualizar lista", icon::ARROWS_CLOCKWISE))).clicked() {
                            sv.drive_refresh_list();
                        }
                        if ui.add(Btn::danger(format!("{}  Desconectar", icon::POWER))).clicked() {
                            sv.drive_disconnect();
                            sv.notify("Google Drive desconectado.");
                        }
                    }
                });
                if sv.drive.is_some() {
                    let mut auto = sv.drive_auto_enabled();
                    if theme::checkbox(ui, &mut auto, "Enviar automaticamente junto com o backup diário").changed() {
                        let _ = sv.db.set_setting("gdrive_auto", if auto { "1" } else { "0" });
                    }
                    ui.add_space(4.0);
                    let files = sv.drive_files.clone();
                    for f in &files {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}  {}", icon::CLOUD, f.name));
                            let created = f.created.get(..16).unwrap_or("").replace('T', " ");
                            ui.label(theme::faint(format!("{created} UTC  {}", format_size(f.size))));
                            if ui
                                .add_enabled(!sv.drive_busy, Btn::secondary(format!("{}  Restaurar", icon::CLOUD_ARROW_DOWN)).compact())
                                .clicked()
                            {
                                sv.drive_download(f.id.clone());
                                sv.notify("Baixando o backup...");
                            }
                        });
                    }
                }
            });
        });

        let ctx = ui.ctx().clone();
        if let Some(form) = &mut self.password_form
            && form.show(&ctx, sv) != Outcome::Open
        {
            self.password_form = None;
        }
        if let Some(form) = &mut self.drive_form
            && form.show(&ctx, sv) != Outcome::Open
        {
            self.drive_form = None;
        }
        if let Some(form) = &mut self.file_form {
            match form.show(&ctx) {
                Ok(None) => {}
                Ok(Some((path, data))) => {
                    self.restore = Some(RestoreForm::new(path, data, sv));
                    self.file_form = None;
                }
                Err(()) => self.file_form = None,
            }
        }
        if let Some(form) = &mut self.restore
            && form.show(&ctx, sv) != Outcome::Open
        {
            self.restore = None;
        }
    }
}
