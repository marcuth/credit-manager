use super::View;
use super::theme::{self, Btn, Tone};
use super::dialogs::{DialogAction, Outcome, field, form_dialog, secret_field};
use crate::billing::channel::{ChannelKind, ChannelStatus, MessageChannel, whatsapp_number};
use crate::services::Services;
use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use qrcode::{Color as QrColor, QrCode};

#[derive(Default)]
pub struct ChannelsView {
    loaded: bool,
    auto_whatsapp: bool,
    auto_telegram: bool,
    telegram_form: Option<TelegramForm>,
}

/// Diálogo de login da conta do Telegram. A etapa mostrada vem do status do canal:
/// dados da aplicação e celular -> código -> senha (se houver verificação em duas etapas).
struct TelegramForm {
    api_id: String,
    api_hash: String,
    phone: String,
    code: String,
    password: String,
    reveal: bool,
    error: Option<String>,
}

impl TelegramForm {
    fn load(sv: &Services) -> Self {
        let get = |k: &str| sv.db.get_setting(k).ok().flatten().unwrap_or_default();
        Self {
            api_id: get("telegram_api_id"),
            api_hash: sv.secret("telegram_api_hash"),
            phone: get("telegram_phone"),
            code: String::new(),
            password: String::new(),
            reveal: true,
            error: None,
        }
    }

    fn show(&mut self, ctx: &egui::Context, sv: &mut Services) -> Outcome {
        let status = sv.channels.telegram.status();
        if status == ChannelStatus::Ready {
            return Outcome::Saved;
        }
        let error = self.error.clone().or_else(|| sv.channels.telegram.last_error());
        let (title, submit) = match &status {
            ChannelStatus::AwaitingCode => ("Código de login", "Entrar"),
            ChannelStatus::AwaitingPassword(_) => ("Verificação em duas etapas", "Entrar"),
            _ => ("Conta do Telegram", "Enviar código"),
        };
        let action = form_dialog(
            ctx,
            "telegram_form",
            &format!("{}  {title}", icon::TELEGRAM_LOGO),
            submit,
            error.as_deref(),
            |ui| match &status {
                ChannelStatus::Connecting => {
                    ui.label(theme::dim("Conectando ao Telegram..."));
                }
                ChannelStatus::AwaitingCode => {
                    ui.label("Digite o código que o Telegram enviou para o seu aplicativo (ou por SMS).");
                    ui.add_space(6.0);
                    field(ui, "Código", &mut self.code, "12345", false);
                }
                ChannelStatus::AwaitingPassword(hint) => {
                    ui.label("Esta conta tem verificação em duas etapas. Informe a senha.");
                    if let Some(h) = hint {
                        ui.label(theme::faint(format!("Dica: {h}")));
                    }
                    ui.add_space(6.0);
                    secret_field(ui, "Senha", &mut self.password, "", &mut self.reveal);
                }
                _ => {
                    ui.label(
                        "Entre com a sua conta do Telegram para enviar mensagens a qualquer cliente. \
                         Crie um aplicativo em my.telegram.org (API development tools) e copie o api_id e o api_hash.",
                    );
                    ui.add_space(6.0);
                    field(ui, "api_id", &mut self.api_id, "1234567", false);
                    secret_field(ui, "api_hash", &mut self.api_hash, "0123456789abcdef...", &mut self.reveal);
                    field(ui, "Seu celular (com DDD)", &mut self.phone, "(11) 91234-5678", false);
                }
            },
        );
        match action {
            DialogAction::None => Outcome::Open,
            DialogAction::Cancel => Outcome::Cancelled,
            DialogAction::Submit => {
                self.error = None;
                match &status {
                    ChannelStatus::Connecting => {}
                    ChannelStatus::AwaitingCode => sv.channels.telegram.submit_code(std::mem::take(&mut self.code)),
                    ChannelStatus::AwaitingPassword(_) => {
                        sv.channels.telegram.submit_password(std::mem::take(&mut self.password))
                    }
                    _ => {
                        let Ok(api_id) = self.api_id.trim().parse::<i32>() else {
                            self.error = Some("O api_id deve ser um número.".into());
                            return Outcome::Open;
                        };
                        let api_hash = self.api_hash.trim().to_string();
                        if api_hash.is_empty() {
                            self.error = Some("Informe o api_hash.".into());
                            return Outcome::Open;
                        }
                        let Some(number) = whatsapp_number(&self.phone) else {
                            self.error = Some("Celular inválido. Use o DDD, ex: (11) 91234-5678.".into());
                            return Outcome::Open;
                        };
                        let saved = [
                            sv.db.set_setting("telegram_api_id", &api_id.to_string()),
                            sv.set_secret("telegram_api_hash", &api_hash),
                            sv.db.set_setting("telegram_phone", self.phone.trim()),
                            sv.db.set_setting("telegram_enabled", "1"),
                        ];
                        if let Some(Err(e)) = saved.into_iter().find(|r| r.is_err()) {
                            self.error = Some(format!("Não foi possível salvar: {e}"));
                            return Outcome::Open;
                        }
                        sv.channels.telegram.start_login(api_id, api_hash, format!("+{number}"));
                    }
                }
                Outcome::Open
            }
        }
    }
}

fn status_label(ui: &mut egui::Ui, status: &ChannelStatus) {
    let (glyph, text, tone) = match status {
        ChannelStatus::NotConfigured => (icon::PLUGS, "Não configurado".to_string(), Tone::Neutral),
        ChannelStatus::Disconnected => (icon::PLUGS, "Desconectado".to_string(), Tone::Neutral),
        ChannelStatus::Connecting => (icon::ARROWS_CLOCKWISE, "Conectando...".to_string(), Tone::Warning),
        ChannelStatus::WaitingQr(_) => (icon::QR_CODE, "Aguardando leitura do QR code".to_string(), Tone::Warning),
        ChannelStatus::NeedsLogin => (icon::KEY, "Falta entrar na conta".to_string(), Tone::Warning),
        ChannelStatus::AwaitingCode => (icon::KEY, "Aguardando o código de login".to_string(), Tone::Warning),
        ChannelStatus::AwaitingPassword(_) => (icon::KEY, "Aguardando a senha".to_string(), Tone::Warning),
        ChannelStatus::Ready => (icon::CHECK_CIRCLE, "Conectado".to_string(), Tone::Success),
        ChannelStatus::Error(e) => (icon::WARNING, format!("Erro: {e}"), Tone::Danger),
    };
    theme::badge(ui, format!("{glyph}  {text}"), tone);
}

/// Desenha o QR code com retângulos (sem precisar de imagem/textura).
fn draw_qr(ui: &mut egui::Ui, payload: &str) {
    let Ok(code) = QrCode::new(payload.as_bytes()) else {
        ui.label("Não foi possível gerar o QR code.");
        return;
    };
    let n = code.width();
    let quiet = 2usize;
    let module = 6.0;
    let side = (n + quiet * 2) as f32 * module;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    for (i, c) in code.to_colors().into_iter().enumerate() {
        if c == QrColor::Dark {
            let (x, y) = ((i % n + quiet) as f32, (i / n + quiet) as f32);
            let min = rect.min + egui::vec2(x * module, y * module);
            painter.rect_filled(egui::Rect::from_min_size(min, egui::vec2(module, module)), 0.0, Color32::BLACK);
        }
    }
}

impl View for ChannelsView {
    fn ui(&mut self, ui: &mut egui::Ui, sv: &mut Services) {
        if !self.loaded {
            self.auto_whatsapp = sv.auto_send_enabled(ChannelKind::WhatsApp);
            self.auto_telegram = sv.auto_send_enabled(ChannelKind::Telegram);
            self.loaded = true;
        }

        ui.heading(format!("{}  Canais", icon::PLUGS_CONNECTED));
        ui.label("Meios usados para enviar o link de cobrança aos clientes.");
        ui.add_space(8.0);

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            // ---------------- WhatsApp ----------------
            theme::card(ui, |ui| {
                let status = sv.channels.whatsapp.status();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}  WhatsApp", icon::WHATSAPP_LOGO)).heading());
                    status_label(ui, &status);
                });
                ui.label(
                    RichText::new(format!(
                        "{}  Usa uma biblioteca não oficial (protocolo do WhatsApp Web). Isso pode violar os \
                         Termos de Uso do WhatsApp e levar ao bloqueio do número. Prefira um número dedicado \
                         e evite enviar mensagens em massa.",
                        icon::WARNING
                    ))
                    .color(theme::WARNING),
                );
                match &status {
                    ChannelStatus::WaitingQr(code) => {
                        ui.label("No celular: WhatsApp > Aparelhos conectados > Conectar um aparelho, e leia o código:");
                        draw_qr(ui, code);
                    }
                    ChannelStatus::Connecting | ChannelStatus::Ready => {}
                    _ => {
                        let label = if sv.channels.whatsapp.has_session() { "Conectar" } else { "Conectar (ler QR code)" };
                        if ui.add(Btn::primary(format!("{}  {label}", icon::PLUGS_CONNECTED))).clicked() {
                            let _ = sv.db.set_setting("whatsapp_enabled", "1");
                            sv.channels.whatsapp.connect();
                        }
                    }
                }
                ui.horizontal(|ui| {
                    if matches!(status, ChannelStatus::Ready | ChannelStatus::Connecting | ChannelStatus::WaitingQr(_))
                        && ui.add(Btn::secondary(format!("{}  Desconectar", icon::POWER))).clicked()
                    {
                        let _ = sv.db.set_setting("whatsapp_enabled", "0");
                        sv.channels.whatsapp.disconnect();
                    }
                    if sv.channels.whatsapp.has_session()
                        && ui.add(Btn::danger(format!("{}  Desvincular aparelho (apagar sessão)", icon::TRASH))).clicked()
                    {
                        let _ = sv.db.set_setting("whatsapp_enabled", "0");
                        sv.channels.whatsapp.reset_session();
                        sv.notify("Sessão do WhatsApp apagada.");
                    }
                });
                if theme::checkbox(ui, &mut self.auto_whatsapp, "Enviar automaticamente ao gerar uma cobrança").changed() {
                    let _ = sv.db.set_setting(&ChannelKind::WhatsApp.auto_send_setting(), if self.auto_whatsapp { "1" } else { "0" });
                }
            });
            ui.add_space(10.0);

            // ---------------- Telegram ----------------
            theme::card(ui, |ui| {
                let status = sv.channels.telegram.status();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}  Telegram", icon::TELEGRAM_LOGO)).heading());
                    status_label(ui, &status);
                });
                ui.label(
                    RichText::new(format!(
                        "{}  Envia como a sua conta pessoal do Telegram, para qualquer número que tenha Telegram. \
                         Mensagens em massa ou não solicitadas podem fazer o Telegram limitar ou banir a conta. \
                         Cada número enviado é salvo como contato na sua conta.",
                        icon::WARNING
                    ))
                    .color(theme::WARNING),
                );
                ui.horizontal(|ui| {
                    if status != ChannelStatus::Ready
                        && ui.add(Btn::primary(format!("{}  Conectar conta", icon::PLUGS_CONNECTED))).clicked()
                    {
                        self.telegram_form = Some(TelegramForm::load(sv));
                        if sv.channels.telegram.has_session()
                            && let Some(id) = sv.db.get_setting("telegram_api_id").ok().flatten().and_then(|v| v.parse().ok())
                        {
                            let _ = sv.db.set_setting("telegram_enabled", "1");
                            sv.channels.telegram.connect(id);
                        }
                    }
                    if status == ChannelStatus::Ready && ui.add(Btn::secondary(format!("{}  Desconectar", icon::POWER))).clicked() {
                        let _ = sv.db.set_setting("telegram_enabled", "0");
                        sv.channels.telegram.disconnect();
                    }
                    if sv.channels.telegram.has_session()
                        && ui.add(Btn::danger(format!("{}  Sair da conta (apagar sessão)", icon::TRASH))).clicked()
                    {
                        let _ = sv.db.set_setting("telegram_enabled", "0");
                        sv.channels.telegram.sign_out();
                        sv.notify("Sessão do Telegram apagada.");
                    }
                });
                if theme::checkbox(ui, &mut self.auto_telegram, "Enviar automaticamente ao gerar uma cobrança").changed() {
                    let _ = sv.db.set_setting(&ChannelKind::Telegram.auto_send_setting(), if self.auto_telegram { "1" } else { "0" });
                }
            });
        });

        if let Some(form) = &mut self.telegram_form {
            let ctx = ui.ctx().clone();
            if form.show(&ctx, sv) != Outcome::Open {
                self.telegram_form = None;
            }
        }
    }
}
