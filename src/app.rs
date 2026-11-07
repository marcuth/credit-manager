use crate::billing::PaymentStatus;
use crate::billing::channel::{ChannelKind, ChannelStatus, charge_message};
use crate::crypto::Vault;
use crate::db::Db;
use crate::security;
use crate::services::{Event, PendingRestore, Services};
use crate::ui::lock_view::LockScreen;
use crate::ui::{APP_NAME, Section, View, create_view, theme};
use eframe::egui;
use std::sync::mpsc::{self, Receiver};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const POLL_EVERY: Duration = Duration::from_secs(30);
const BACKUP_CHECK_EVERY: Duration = Duration::from_secs(60);

/// O app tem dois estágios: a tela de senha e a área de trabalho. Os serviços (canais, provedor
/// de pagamento, Drive) só são criados depois de desbloquear, porque dependem dos tokens cifrados.
pub struct App {
    stage: Stage,
    data_dir: PathBuf,
}

enum Stage {
    Locked(Box<LockScreen>),
    Open(Box<Workspace>),
    Failed(String),
}

impl App {
    pub fn new(db: Db, data_dir: &Path) -> std::io::Result<Self> {
        // Quem escolheu não usar senha entra direto.
        let stage = if security::is_skipped(&db) && !security::is_configured(&db) {
            Stage::Open(Box::new(Workspace::new(db, data_dir, None)?))
        } else {
            Stage::Locked(Box::new(LockScreen::new(db)))
        };
        Ok(Self { stage, data_dir: data_dir.to_path_buf() })
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Stage::Open(ws) = &mut self.stage {
            ws.logic(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        match &mut self.stage {
            Stage::Open(ws) => ws.draw(ui),
            Stage::Locked(lock) => {
                if let Some((db, vault)) = lock.show(ui) {
                    self.stage = match Workspace::new(db, &self.data_dir, vault) {
                        Ok(ws) => Stage::Open(Box::new(ws)),
                        Err(e) => Stage::Failed(format!("Não foi possível iniciar o sistema: {e}")),
                    };
                }
            }
            Stage::Failed(message) => {
                egui::CentralPanel::default().frame(theme::content_frame()).show(ui, |ui| {
                    ui.label(egui::RichText::new(message.as_str()).color(theme::DANGER));
                });
            }
        }
    }
}

struct Workspace {
    services: Services,
    events: Receiver<Event>,
    section: Section,
    view: Box<dyn View>,
    last_poll: Instant,
    last_backup_check: Option<Instant>,
}

impl Workspace {
    fn new(db: Db, data_dir: &Path, vault: Option<Vault>) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let section = Section::Transactions;
        let mut services = Services::new(db, tx, data_dir, vault)?;
        services.check_pending();
        if services.drive.is_some() {
            services.drive_refresh_list();
        }
        Ok(Self {
            services,
            events: rx,
            section,
            view: create_view(section),
            last_poll: Instant::now(),
            last_backup_check: None,
        })
    }

    fn logic(&mut self, ctx: &egui::Context) {
        self.handle_events(ctx);
        if self.last_poll.elapsed() >= POLL_EVERY {
            self.last_poll = Instant::now();
            self.services.check_pending();
        }
        // Backup automático: confere na abertura e depois a cada minuto se já está na hora.
        if self.last_backup_check.is_none_or(|t| t.elapsed() >= BACKUP_CHECK_EVERY) {
            self.last_backup_check = Some(Instant::now());
            if self.services.backup_due() {
                self.services.run_backup();
            }
        }
        // Acorda a cada segundo para receber resultados de rede e disparar as rotinas periódicas.
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            let sv = &mut self.services;
            match event {
                Event::CheckoutCreated { customer_id, amount_cents, external_ref, result } => match result {
                    Ok(checkout) => match sv.db.add_charge(customer_id, amount_cents, &external_ref, &checkout.url) {
                        Ok(_) => {
                            sv.changed();
                            sv.notify("Link de pagamento gerado.");
                            // Envio automático pelos canais que estão prontos e alcançam o cliente.
                            let customer = sv.db.list_customers("").unwrap_or_default().into_iter().find(|c| c.id == customer_id);
                            if let Some(c) = customer {
                                let text = charge_message(&c.name, amount_cents, &checkout.url);
                                for kind in ChannelKind::ALL {
                                    let ch = sv.channels.get(kind);
                                    if sv.auto_send_enabled(kind) && ch.status() == ChannelStatus::Ready && ch.can_reach(&c) {
                                        sv.send_via(kind, c.clone(), text.clone());
                                    }
                                }
                            }
                        }
                        Err(e) => sv.notify(e.to_string()),
                    },
                    Err(e) => sv.notify(e.to_string()),
                },
                Event::PaymentChecked { charge_id, result } => match result {
                    Ok(PaymentStatus::Approved { payment_id }) => match sv.db.mark_charge_paid(charge_id, &payment_id) {
                        Ok(true) => {
                            sv.changed();
                            sv.notify("Pagamento recebido e registrado!");
                        }
                        Ok(false) => {}
                        Err(e) => sv.notify(e.to_string()),
                    },
                    Ok(PaymentStatus::Pending) => {}
                    Err(e) => sv.notify(e.to_string()),
                },
                Event::MessageSent { channel, customer, result } => match result {
                    Ok(()) => sv.notify(format!("Mensagem enviada para {customer} pelo {}.", channel.label())),
                    Err(e) => sv.notify(format!("{}: não foi possível enviar para {customer}: {e}", channel.label())),
                },
                Event::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                Event::DriveLogin(result) => {
                    sv.drive_busy = false;
                    match result {
                        Ok(auth) => {
                            let saved = [
                                sv.db.set_setting("gdrive_client_id", &auth.client_id),
                                sv.set_secret("gdrive_client_secret", &auth.client_secret),
                                sv.set_secret("gdrive_refresh_token", &auth.refresh_token),
                            ];
                            match saved.into_iter().find_map(Result::err) {
                                None => {
                                    sv.drive = Some(auth);
                                    sv.notify("Conectado ao Google Drive.");
                                    sv.drive_refresh_list();
                                }
                                Some(e) => sv.notify(format!("Não foi possível salvar a conexão: {e}")),
                            }
                        }
                        Err(e) => sv.notify(e.to_string()),
                    }
                }
                Event::DriveList(result) => {
                    sv.drive_busy = false;
                    match result {
                        Ok(files) => sv.drive_files = files,
                        Err(e) => sv.notify(e.to_string()),
                    }
                }
                Event::DriveUploaded(result) => {
                    sv.drive_busy = false;
                    match result {
                        Ok(()) => {
                            sv.notify("Backup enviado ao Google Drive.");
                            sv.drive_refresh_list();
                        }
                        Err(e) => sv.notify(e.to_string()),
                    }
                }
                Event::DriveDownloaded(result) => {
                    sv.drive_busy = false;
                    match result {
                        Ok(data) => sv.pending_restore = Some(PendingRestore { source: "Google Drive".into(), data }),
                        Err(e) => sv.notify(e.to_string()),
                    }
                }
            }
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("sidebar").resizable(false).exact_size(210.0).frame(theme::sidebar_frame()).show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(theme::title(format!("{}  {APP_NAME}", egui_phosphor::regular::WALLET)));
            ui.add_space(14.0);
            for s in Section::ALL {
                if theme::nav_item(ui, self.section == s, format!("{}  {}", s.icon(), s.label())).clicked()
                    && self.section != s
                {
                    self.section = s;
                    self.view = create_view(s);
                }
            }
        });

        egui::Panel::bottom("status").frame(theme::status_frame()).show(ui, |ui| {
            ui.horizontal(|ui| {
                match self.services.status.as_deref() {
                    Some(msg) => ui.label(theme::dim(msg)),
                    None => ui.label(theme::faint("Pronto")),
                };
                if self.services.status.is_some() && ui.add(theme::Btn::secondary("OK").compact()).clicked() {
                    self.services.status = None;
                }
            });
        });

        egui::CentralPanel::default().frame(theme::content_frame()).show(ui, |ui| {
            self.view.ui(ui, &mut self.services);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        ctx.set_fonts(fonts);
        crate::ui::theme::apply(&ctx);
        ctx
    }

    fn run_frames(ctx: &egui::Context, frames: usize, mut ui_fn: impl FnMut(&mut egui::Ui)) {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        for i in 0..frames {
            let raw = egui::RawInput { screen_rect: Some(screen), time: Some(i as f64 * 0.016), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| ui_fn(ui));
            out.textures_delta.clear();
        }
    }

    /// Desenha o app inteiro, com o tema, algumas vezes em cada seção, para pegar pânicos de layout.
    #[test]
    fn every_section_renders_without_panicking() {
        let dir = std::env::temp_dir().join(format!("credit-manager-test-smoke-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut ws = Workspace::new(Db::open_in_memory().unwrap(), &dir, None).unwrap();
        ws.services.db.add_customer("Ana", "(11) 91234-5678", "").unwrap();
        ws.services.changed();

        let ctx = context();
        for section in Section::ALL {
            ws.section = section;
            ws.view = create_view(section);
            run_frames(&ctx, 4, |ui| ws.draw(ui));
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn lock_screen_renders_and_rejects_a_wrong_password() {
        let db = Db::open_in_memory().unwrap();
        security::create(&db, "senha-certa").unwrap();
        let mut lock = LockScreen::new(db);
        let ctx = context();
        run_frames(&ctx, 3, |ui| {
            assert!(lock.show(ui).is_none());
        });
    }

    #[test]
    fn first_run_without_password_choice_asks_for_one() {
        let dir = std::env::temp_dir().join(format!("credit-manager-test-first-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let app = App::new(Db::open_in_memory().unwrap(), &dir).unwrap();
        assert!(matches!(app.stage, Stage::Locked(_)));

        let db = Db::open_in_memory().unwrap();
        security::skip(&db).unwrap();
        let app = App::new(db, &dir).unwrap();
        assert!(matches!(app.stage, Stage::Open(_)), "quem optou por não usar senha entra direto");
        let _ = std::fs::remove_dir_all(dir);
    }
}
