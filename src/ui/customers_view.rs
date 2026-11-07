use super::View;
use crate::db::{Customer, Kind, Transaction};
use crate::money::{format_cents, parse_cents};
use crate::services::Services;
use eframe::egui::{self, Color32, RichText};

#[derive(Default)]
pub struct CustomersView {
    revision: Option<u64>,
    search: String,
    customers: Vec<Customer>,
    selected: Option<i64>,
    history: Vec<Transaction>,
    // formulário de novo cliente
    new_name: String,
    new_phone: String,
    new_email: String,
    // formulário de transação do cliente selecionado
    amount: String,
    note: String,
}

impl CustomersView {
    fn reload(&mut self, sv: &Services) {
        self.customers = sv.db.list_customers(&self.search).unwrap_or_default();
        self.history = self.selected.map(|id| sv.db.list_transactions(id).unwrap_or_default()).unwrap_or_default();
        self.revision = Some(sv.revision);
    }
}

fn balance_text(cents: i64) -> RichText {
    match cents {
        c if c > 0 => RichText::new(format!("Deve {}", format_cents(c))).color(Color32::from_rgb(200, 60, 60)),
        c if c < 0 => RichText::new(format!("Crédito {}", format_cents(-c))).color(Color32::from_rgb(40, 150, 80)),
        _ => RichText::new("Em dia"),
    }
}

impl View for CustomersView {
    fn ui(&mut self, ui: &mut egui::Ui, sv: &mut Services) {
        if self.revision != Some(sv.revision) {
            self.reload(sv);
        }

        ui.heading("Clientes");
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(RichText::new("Novo cliente").strong());
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text("Nome").desired_width(200.0));
                ui.add(egui::TextEdit::singleline(&mut self.new_phone).hint_text("Celular com DDD").desired_width(150.0));
                ui.add(egui::TextEdit::singleline(&mut self.new_email).hint_text("E-mail").desired_width(200.0));
                if ui.button("Cadastrar").clicked() {
                    if self.new_name.trim().is_empty() {
                        sv.notify("Informe o nome do cliente.");
                    } else {
                        match sv.db.add_customer(&self.new_name, &self.new_phone, &self.new_email) {
                            Ok(id) => {
                                self.selected = Some(id);
                                self.new_name.clear();
                                self.new_phone.clear();
                                self.new_email.clear();
                                sv.changed();
                                sv.notify("Cliente cadastrado.");
                            }
                            Err(e) => sv.notify(e.to_string()),
                        }
                    }
                }
            });
        });
        ui.add_space(8.0);

        ui.columns(2, |cols| {
            // Lista
            let ui = &mut cols[0];
            if ui
                .add(egui::TextEdit::singleline(&mut self.search).hint_text("Buscar por nome").desired_width(f32::INFINITY))
                .changed()
            {
                self.reload(sv);
            }
            egui::ScrollArea::vertical().id_salt("customers").auto_shrink(false).show(ui, |ui| {
                let mut clicked = None;
                for c in &self.customers {
                    let selected = self.selected == Some(c.id);
                    ui.horizontal(|ui| {
                        if ui.selectable_label(selected, &c.name).clicked() {
                            clicked = Some(c.id);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(balance_text(c.balance_cents));
                        });
                    });
                }
                if let Some(id) = clicked {
                    self.selected = Some(id);
                    self.reload(sv);
                }
            });

            // Detalhe
            let ui = &mut cols[1];
            let Some(c) = self.customers.iter().find(|c| Some(c.id) == self.selected).cloned() else {
                ui.label("Selecione um cliente para ver o histórico.");
                return;
            };
            ui.label(RichText::new(&c.name).heading());
            ui.label(format!("Celular: {}   E-mail: {}", or_dash(&c.phone), or_dash(&c.email)));
            ui.label(balance_text(c.balance_cents).size(18.0));
            ui.horizontal(|ui| {
                if c.telegram_chat_id.is_some() {
                    ui.colored_label(Color32::from_rgb(40, 150, 80), "Telegram vinculado");
                } else if let Some(bot) = sv.channels.telegram.username() {
                    if ui.button("Copiar link do Telegram").clicked() {
                        match sv.db.ensure_telegram_link_code(c.id) {
                            Ok(code) => {
                                ui.ctx().copy_text(format!("https://t.me/{bot}?start={code}"));
                                sv.notify("Link copiado. Envie ao cliente para ele vincular o Telegram.");
                            }
                            Err(e) => sv.notify(e.to_string()),
                        }
                    }
                } else {
                    ui.label("Telegram: configure o bot na tela Canais.");
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.amount).hint_text("Valor").desired_width(100.0));
                ui.add(egui::TextEdit::singleline(&mut self.note).hint_text("Anotação").desired_width(160.0));
            });
            ui.horizontal(|ui| {
                for (label, kind) in [("Adicionar dívida", Kind::Debt), ("Adicionar pagamento", Kind::Payment)] {
                    if ui.button(label).clicked() {
                        match parse_cents(&self.amount).filter(|v| *v > 0) {
                            None => sv.notify("Valor inválido."),
                            Some(cents) => match sv.db.add_transaction(c.id, kind, cents, &self.note) {
                                Ok(()) => {
                                    self.amount.clear();
                                    self.note.clear();
                                    sv.changed();
                                }
                                Err(e) => sv.notify(e.to_string()),
                            },
                        }
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("history").auto_shrink(false).show(ui, |ui| {
                for t in &self.history {
                    let (label, color, sign) = match t.kind {
                        Kind::Debt => ("Dívida", Color32::from_rgb(200, 60, 60), "+"),
                        Kind::Payment => ("Pagamento", Color32::from_rgb(40, 150, 80), "-"),
                    };
                    ui.horizontal(|ui| {
                        ui.label(&t.created_at);
                        ui.label(RichText::new(format!("{label} {sign} {}", format_cents(t.amount_cents))).color(color));
                        ui.label(&t.note);
                    });
                }
            });
        });
    }
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "—" } else { s }
}
