use super::View;
use super::dialogs::{CustomerForm, Outcome, TransactionForm};
use crate::db::{Customer, Kind, Transaction};
use crate::money::format_cents;
use crate::services::Services;
use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;

#[derive(Default)]
pub struct CustomersView {
    revision: Option<u64>,
    search: String,
    customers: Vec<Customer>,
    selected: Option<i64>,
    history: Vec<Transaction>,
    customer_form: Option<CustomerForm>,
    tx_form: Option<TransactionForm>,
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

        ui.horizontal(|ui| {
            ui.heading(format!("{}  Clientes", icon::USERS));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(format!("{}  Novo cliente", icon::USER_PLUS)).clicked() {
                    self.customer_form = Some(CustomerForm::default());
                }
            });
        });
        ui.add_space(8.0);

        ui.columns(2, |cols| {
            // Lista
            let ui = &mut cols[0];
            let search = egui::TextEdit::singleline(&mut self.search)
                .hint_text(format!("{}  Buscar por nome", icon::MAGNIFYING_GLASS))
                .desired_width(f32::INFINITY);
            if ui.add(search).changed() {
                self.reload(sv);
            }
            egui::ScrollArea::vertical().id_salt("customers").auto_shrink(false).show(ui, |ui| {
                let mut clicked = None;
                for c in &self.customers {
                    let selected = self.selected == Some(c.id);
                    ui.horizontal(|ui| {
                        if ui.selectable_label(selected, format!("{}  {}", icon::USER, c.name)).clicked() {
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
            ui.label(format!("{} {}    {} {}", icon::PHONE, or_dash(&c.phone), icon::ENVELOPE_SIMPLE, or_dash(&c.email)));
            ui.label(balance_text(c.balance_cents).size(18.0));
            ui.horizontal_wrapped(|ui| {
                if ui.button(format!("{}  Adicionar dívida", icon::ARROW_UP)).clicked() {
                    self.tx_form = Some(TransactionForm::for_customer(c.id, Kind::Debt));
                }
                if ui.button(format!("{}  Adicionar pagamento", icon::ARROW_DOWN)).clicked() {
                    self.tx_form = Some(TransactionForm::for_customer(c.id, Kind::Payment));
                }
                if c.telegram_chat_id.is_some() {
                    ui.colored_label(Color32::from_rgb(40, 150, 80), format!("{}  Telegram vinculado", icon::CHECK_CIRCLE));
                } else if let Some(bot) = sv.channels.telegram.username() {
                    if ui.button(format!("{}  Copiar link do Telegram", icon::TELEGRAM_LOGO)).clicked() {
                        match sv.db.ensure_telegram_link_code(c.id) {
                            Ok(code) => {
                                ui.ctx().copy_text(format!("https://t.me/{bot}?start={code}"));
                                sv.notify("Link copiado. Envie ao cliente para ele vincular o Telegram.");
                            }
                            Err(e) => sv.notify(e.to_string()),
                        }
                    }
                } else {
                    ui.label(format!("{}  Telegram: configure o bot em Canais.", icon::TELEGRAM_LOGO));
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("history").auto_shrink(false).show(ui, |ui| {
                for t in &self.history {
                    let (label, glyph, color, sign) = match t.kind {
                        Kind::Debt => ("Dívida", icon::ARROW_UP, Color32::from_rgb(200, 60, 60), "+"),
                        Kind::Payment => ("Pagamento", icon::ARROW_DOWN, Color32::from_rgb(40, 150, 80), "-"),
                    };
                    ui.horizontal(|ui| {
                        ui.label(&t.created_at);
                        ui.label(RichText::new(format!("{glyph} {label} {sign} {}", format_cents(t.amount_cents))).color(color));
                        ui.label(&t.note);
                    });
                }
            });
        });

        let ctx = ui.ctx().clone();
        if let Some(form) = &mut self.customer_form {
            match form.show(&ctx, sv) {
                Outcome::Open => {}
                outcome => {
                    if outcome == Outcome::Saved {
                        self.selected = form.created_id;
                    }
                    self.customer_form = None;
                }
            }
        }
        if let Some(form) = &mut self.tx_form
            && form.show(&ctx, sv, &self.customers) != Outcome::Open
        {
            self.tx_form = None;
        }
    }
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "—" } else { s }
}
