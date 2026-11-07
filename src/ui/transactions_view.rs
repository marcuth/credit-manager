use super::{View, customer_combo};
use crate::db::{Customer, Kind, TransactionEntry};
use crate::money::{format_cents, parse_cents};
use crate::services::Services;
use eframe::egui::{self, Color32, RichText};

pub struct TransactionsView {
    revision: Option<u64>,
    customers: Vec<Customer>,
    entries: Vec<TransactionEntry>,
    customer: Option<i64>,
    kind: Kind,
    amount: String,
    note: String,
}

impl Default for TransactionsView {
    fn default() -> Self {
        Self {
            revision: None,
            customers: vec![],
            entries: vec![],
            customer: None,
            kind: Kind::Debt,
            amount: String::new(),
            note: String::new(),
        }
    }
}

impl View for TransactionsView {
    fn ui(&mut self, ui: &mut egui::Ui, sv: &mut Services) {
        if self.revision != Some(sv.revision) {
            self.customers = sv.db.list_customers("").unwrap_or_default();
            self.entries = sv.db.list_all_transactions(500).unwrap_or_default();
            self.revision = Some(sv.revision);
        }

        ui.heading("Transações");
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(RichText::new("Nova transação").strong());
            ui.horizontal_wrapped(|ui| {
                customer_combo(ui, "tx_customer", &self.customers, &mut self.customer);
                ui.radio_value(&mut self.kind, Kind::Debt, "Dívida (débito)");
                ui.radio_value(&mut self.kind, Kind::Payment, "Pagamento (crédito)");
            });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.amount).hint_text("Valor, ex: 25,50").desired_width(140.0));
                ui.add(egui::TextEdit::singleline(&mut self.note).hint_text("Anotação (opcional)").desired_width(260.0));
                if ui.button("Adicionar").clicked() {
                    match (self.customer, parse_cents(&self.amount).filter(|c| *c > 0)) {
                        (None, _) => sv.notify("Escolha um cliente."),
                        (_, None) => sv.notify("Valor inválido."),
                        (Some(id), Some(cents)) => match sv.db.add_transaction(id, self.kind, cents, &self.note) {
                            Ok(()) => {
                                self.amount.clear();
                                self.note.clear();
                                sv.changed();
                                sv.notify("Transação registrada.");
                            }
                            Err(e) => sv.notify(e.to_string()),
                        },
                    }
                }
            });
        });
        ui.add_space(8.0);

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            egui::Grid::new("tx_grid").striped(true).num_columns(5).spacing([24.0, 6.0]).show(ui, |ui| {
                for h in ["Data", "Cliente", "Tipo", "Valor", "Anotação"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for e in &self.entries {
                    let (label, color, sign) = match e.tx.kind {
                        Kind::Debt => ("Dívida", Color32::from_rgb(200, 60, 60), "+"),
                        Kind::Payment => ("Pagamento", Color32::from_rgb(40, 150, 80), "-"),
                    };
                    ui.label(&e.tx.created_at);
                    ui.label(&e.customer_name);
                    ui.label(RichText::new(label).color(color));
                    ui.label(RichText::new(format!("{sign} {}", format_cents(e.tx.amount_cents))).color(color));
                    ui.label(&e.tx.note);
                    ui.end_row();
                }
            });
        });
    }
}
