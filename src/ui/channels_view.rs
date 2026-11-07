use super::View;
use super::dialogs::{DialogAction, Outcome, form_dialog, secret_field};
use crate::billing::channel::{ChannelKind, ChannelStatus, MessageChannel};
use crate::services::Services;
use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use qrcode::{Color as QrColor, QrCode};

#[derive(Default)]
pub struct ChannelsView {
    loaded: bool,
    auto_whatsapp: bool,
    auto_telegram: bool,
    token_form: Option<TokenForm>,
}

const GREEN: Color32 = Color32::from_rgb(40, 150, 80);
const AMBER: Color32 = Color32::from_rgb(200, 140, 0);
const RED: Color32 = Color32::from_rgb(200, 60, 60);

/// Diálogo do token do bot do Telegram.
struct TokenForm {
    token: String,
    reveal: bool,
    error: Option<String>,
}

impl TokenForm {
    fn show(&mut self, ctx: &egui::Context, sv: &mut Services) -> Outcome {
        let action = form_dialog(
            ctx,
            "telegram_token_form",
            &format!("{}  Bot do Telegram", icon::TELEGRAM_LOGO),
            "Salvar e conectar",
            self.error.as_deref(),
            |ui| {
                ui.label("1) No Telegram, converse com o @BotFather e crie um bot (/newbot).\n2) Cole abaixo o token que ele devolver.");
                ui.add_space(6.0);
                secret_field(ui, "Token do bot", &mut self.token, "123456:ABC-DEF...", &mut self.reveal);
            },
        );
        match action {
            DialogAction::None => Outcome::Open,
            DialogAction::Cancel => Outcome::Cancelled,
            DialogAction::Submit => {
                if let Err(e) = sv.db.set_setting("telegram_token", self.token.trim()) {
                    self.error = Some(format!("Não foi possível salvar o token: {e}"));
                    return Outcome::Open;
                }
                sv.channels.telegram.configure(&self.token, sv.events.clone());
                Outcome::Saved
            }
        }
    }
}

fn status_label(ui: &mut egui::Ui, status: &ChannelStatus) {
    let (glyph, text, color) = match status {
        ChannelStatus::NotConfigured => (icon::PLUGS, "Não configurado".to_string(), Color32::GRAY),
        ChannelStatus::Disconnected => (icon::PLUGS, "Desconectado".to_string(), Color32::GRAY),
        ChannelStatus::Connecting => (icon::ARROWS_CLOCKWISE, "Conectando...".to_string(), AMBER),
        ChannelStatus::WaitingQr(_) => (icon::QR_CODE, "Aguardando leitura do QR code".to_string(), AMBER),
        ChannelStatus::Ready => (icon::CHECK_CIRCLE, "Conectado".to_string(), GREEN),
        ChannelStatus::Error(e) => (icon::WARNING, format!("Erro: {e}"), RED),
    };
    ui.label(RichText::new(format!("{glyph}  {text}")).color(color).strong());
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
            ui.group(|ui| {
                ui.set_width(ui.available_width());
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
                    .color(AMBER),
                );
                match &status {
                    ChannelStatus::WaitingQr(code) => {
                        ui.label("No celular: WhatsApp > Aparelhos conectados > Conectar um aparelho, e leia o código:");
                        draw_qr(ui, code);
                    }
                    ChannelStatus::Connecting | ChannelStatus::Ready => {}
                    _ => {
                        let label = if sv.channels.whatsapp.has_session() { "Conectar" } else { "Conectar (ler QR code)" };
                        if ui.button(format!("{}  {label}", icon::PLUGS_CONNECTED)).clicked() {
                            let _ = sv.db.set_setting("whatsapp_enabled", "1");
                            sv.channels.whatsapp.connect();
                        }
                    }
                }
                ui.horizontal(|ui| {
                    if matches!(status, ChannelStatus::Ready | ChannelStatus::Connecting | ChannelStatus::WaitingQr(_))
                        && ui.button(format!("{}  Desconectar", icon::POWER)).clicked()
                    {
                        let _ = sv.db.set_setting("whatsapp_enabled", "0");
                        sv.channels.whatsapp.disconnect();
                    }
                    if sv.channels.whatsapp.has_session()
                        && ui.button(format!("{}  Desvincular aparelho (apagar sessão)", icon::TRASH)).clicked()
                    {
                        let _ = sv.db.set_setting("whatsapp_enabled", "0");
                        sv.channels.whatsapp.reset_session();
                        sv.notify("Sessão do WhatsApp apagada.");
                    }
                });
                if ui.checkbox(&mut self.auto_whatsapp, "Enviar automaticamente ao gerar uma cobrança").changed() {
                    let _ = sv.db.set_setting(&ChannelKind::WhatsApp.auto_send_setting(), if self.auto_whatsapp { "1" } else { "0" });
                }
            });
            ui.add_space(10.0);

            // ---------------- Telegram ----------------
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}  Telegram", icon::TELEGRAM_LOGO)).heading());
                    let status = sv.channels.telegram.status();
                    status_label(ui, &status);
                });
                ui.label(
                    "Na tela Clientes, gere o link do Telegram de cada cliente e envie a ele; \
                     quando o cliente abrir o link e tocar em Iniciar, o vínculo é feito.",
                );
                ui.horizontal(|ui| {
                    if ui.button(format!("{}  Configurar bot", icon::KEY)).clicked() {
                        let token = sv.db.get_setting("telegram_token").ok().flatten().unwrap_or_default();
                        self.token_form = Some(TokenForm { token, reveal: true, error: None });
                    }
                    if let Some(name) = sv.channels.telegram.username() {
                        ui.label(format!("Bot ativo: @{name}"));
                    }
                });
                if ui.checkbox(&mut self.auto_telegram, "Enviar automaticamente ao gerar uma cobrança").changed() {
                    let _ = sv.db.set_setting(&ChannelKind::Telegram.auto_send_setting(), if self.auto_telegram { "1" } else { "0" });
                }
            });
        });

        if let Some(form) = &mut self.token_form {
            let ctx = ui.ctx().clone();
            if form.show(&ctx, sv) != Outcome::Open {
                self.token_form = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn token_dialog_accepts_typed_token() {
        let dir = std::env::temp_dir().join("credit-manager-test-token");
        let _ = std::fs::create_dir_all(&dir);
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut sv = Services::new(Db::open_in_memory().unwrap(), tx, &dir).unwrap();
        let mut form = TokenForm { token: String::new(), reveal: true, error: None };

        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let mut frame = |events: Vec<egui::Event>, i: usize| {
            let raw = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(i as f64 * 0.016),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let ctx = ui.ctx().clone();
                form.show(&ctx, &mut sv);
            });
            out.textures_delta.clear();
        };
        for i in 0..4 {
            frame(vec![], i);
        }
        // Tab leva o foco ao primeiro widget do modal: o campo do token.
        let tab = |pressed| egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        frame(vec![tab(true), tab(false)], 4);
        frame(vec![], 5);
        frame(vec![egui::Event::Text("123456:ABC-def".into())], 6);
        frame(vec![], 7);
        assert_eq!(form.token, "123456:ABC-def");
    }
}
