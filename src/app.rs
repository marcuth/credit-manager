use crate::db::Db;
use crate::services::{Event, Services};
use crate::ui::{Section, View, create_view};
use crate::billing::PaymentStatus;
use eframe::egui;
use std::sync::mpsc::{self, Receiver};
use crate::billing::channel::{ChannelKind, ChannelStatus, charge_message};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const POLL_EVERY: Duration = Duration::from_secs(30);

pub struct App {
    services: Services,
    events: Receiver<Event>,
    section: Section,
    view: Box<dyn View>,
    last_poll: Instant,
}

impl App {
    pub fn new(db: Db, data_dir: &Path) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let section = Section::Transactions;
        let mut services = Services::new(db, tx, data_dir)?;
        services.check_pending();
        Ok(Self { services, events: rx, section, view: create_view(section), last_poll: Instant::now() })
    }

    fn handle_events(&mut self) {
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
                Event::TelegramLinked { code, chat_id } => {
                    if let Ok(true) = sv.db.link_telegram(&code, chat_id) {
                        sv.changed();
                        sv.notify("Um cliente vinculou o Telegram.");
                        let tg = sv.channels.telegram.clone();
                        std::thread::spawn(move || {
                            let _ = tg.send_to_chat(chat_id, "Tudo certo! Você receberá os avisos de cobrança por aqui.");
                        });
                    }
                }
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events();
        if self.last_poll.elapsed() >= POLL_EVERY {
            self.last_poll = Instant::now();
            self.services.check_pending();
        }
        // Acorda a cada segundo para receber resultados de rede e disparar a consulta periódica.
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("sidebar").resizable(false).exact_size(200.0).show(ui, |ui| {
            ui.add_space(12.0);
            ui.heading("Gestor de Crédito");
            ui.add_space(16.0);
            for s in Section::ALL {
                let button = egui::Button::new(egui::RichText::new(s.label()).size(16.0))
                    .selected(self.section == s)
                    .min_size(egui::vec2(ui.available_width(), 38.0));
                if ui.add(button).clicked() && self.section != s {
                    self.section = s;
                    self.view = create_view(s);
                }
            }
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                match self.services.status.as_deref() {
                    Some(msg) => ui.label(msg),
                    None => ui.label("Pronto"),
                };
                if self.services.status.is_some() && ui.small_button("OK").clicked() {
                    self.services.status = None;
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            enlarge_controls(ui.style_mut());
            self.view.ui(ui, &mut self.services);
        });
    }
}

/// Controles maiores na área de conteúdo (o sidebar mantém o tamanho padrão).
fn enlarge_controls(style: &mut egui::Style) {
    use egui::{FontId, TextStyle};
    style.spacing.interact_size = egui::vec2(60.0, 40.0);
    style.spacing.button_padding = egui::vec2(16.0, 9.0);
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.combo_width = 260.0;
    style.spacing.icon_width = 22.0;
    for (text_style, size) in [
        (TextStyle::Body, 17.0),
        (TextStyle::Button, 17.0),
        (TextStyle::Small, 14.0),
        (TextStyle::Heading, 26.0),
    ] {
        style.text_styles.insert(text_style, FontId::proportional(size));
    }
}
