use super::View;
use crate::billing::channel::{ChannelKind, ChannelStatus, MessageChannel};
use crate::services::Services;
use eframe::egui::{self, Color32, RichText};
use qrcode::{Color as QrColor, QrCode};

#[derive(Default)]
pub struct ChannelsView {
    loaded: bool,
    telegram_token: String,
    auto_whatsapp: bool,
    auto_telegram: bool,
}

const GREEN: Color32 = Color32::from_rgb(40, 150, 80);
const AMBER: Color32 = Color32::from_rgb(200, 140, 0);
const RED: Color32 = Color32::from_rgb(200, 60, 60);

fn status_label(ui: &mut egui::Ui, status: &ChannelStatus) {
    let (text, color) = match status {
        ChannelStatus::NotConfigured => ("Não configurado".to_string(), Color32::GRAY),
        ChannelStatus::Disconnected => ("Desconectado".to_string(), Color32::GRAY),
        ChannelStatus::Connecting => ("Conectando...".to_string(), AMBER),
        ChannelStatus::WaitingQr(_) => ("Aguardando leitura do QR code".to_string(), AMBER),
        ChannelStatus::Ready => ("Conectado".to_string(), GREEN),
        ChannelStatus::Error(e) => (format!("Erro: {e}"), RED),
    };
    ui.label(RichText::new(text).color(color).strong());
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
            self.telegram_token = sv.db.get_setting("telegram_token").ok().flatten().unwrap_or_default();
            self.auto_whatsapp = sv.auto_send_enabled(ChannelKind::WhatsApp);
            self.auto_telegram = sv.auto_send_enabled(ChannelKind::Telegram);
            self.loaded = true;
        }

        ui.heading("Canais");
        ui.label("Meios usados para enviar o link de cobrança aos clientes.");
        ui.add_space(8.0);

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            // ---------------- WhatsApp ----------------
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("WhatsApp").heading());
                    let status = sv.channels.whatsapp.status();
                    status_label(ui, &status);
                });
                ui.label(
                    RichText::new(
                        "Usa uma biblioteca não oficial (protocolo do WhatsApp Web). Isso pode violar os \
                         Termos de Uso do WhatsApp e levar ao bloqueio do número. Prefira um número dedicado \
                         e evite enviar mensagens em massa.",
                    )
                    .color(AMBER),
                );
                let status = sv.channels.whatsapp.status();
                match &status {
                    ChannelStatus::WaitingQr(code) => {
                        ui.label("No celular: WhatsApp > Aparelhos conectados > Conectar um aparelho, e leia o código:");
                        draw_qr(ui, code);
                    }
                    ChannelStatus::Connecting | ChannelStatus::Ready => {}
                    _ => {
                        let label = if sv.channels.whatsapp.has_session() { "Conectar" } else { "Conectar (ler QR code)" };
                        if ui.button(label).clicked() {
                            let _ = sv.db.set_setting("whatsapp_enabled", "1");
                            sv.channels.whatsapp.connect();
                        }
                    }
                }
                ui.horizontal(|ui| {
                    if matches!(status, ChannelStatus::Ready | ChannelStatus::Connecting | ChannelStatus::WaitingQr(_))
                        && ui.button("Desconectar").clicked()
                    {
                        let _ = sv.db.set_setting("whatsapp_enabled", "0");
                        sv.channels.whatsapp.disconnect();
                    }
                    if sv.channels.whatsapp.has_session() && ui.button("Desvincular aparelho (apagar sessão)").clicked() {
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
                    ui.label(RichText::new("Telegram").heading());
                    let status = sv.channels.telegram.status();
                    status_label(ui, &status);
                });
                ui.label(
                    "1) No Telegram, converse com o @BotFather e crie um bot (/newbot).\n\
                     2) Cole aqui o token que ele devolver e salve.\n\
                     3) Na tela Clientes, gere o link do Telegram de cada cliente e envie a ele; \
                     quando o cliente abrir o link e tocar em Iniciar, o vínculo é feito.",
                );
                ui.horizontal(|ui| {
                    ui.label("Token do bot:");
                    ui.add(egui::TextEdit::singleline(&mut self.telegram_token).password(true).desired_width(380.0));
                    if ui.button("Salvar e conectar").clicked() {
                        let _ = sv.db.set_setting("telegram_token", self.telegram_token.trim());
                        sv.channels.telegram.configure(&self.telegram_token, sv.events.clone());
                    }
                });
                if let Some(name) = sv.channels.telegram.username() {
                    ui.label(format!("Bot ativo: @{name}"));
                }
                if ui.checkbox(&mut self.auto_telegram, "Enviar automaticamente ao gerar uma cobrança").changed() {
                    let _ = sv.db.set_setting(&ChannelKind::Telegram.auto_send_setting(), if self.auto_telegram { "1" } else { "0" });
                }
            });
        });
    }
}
