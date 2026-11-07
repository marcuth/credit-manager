use super::View;
use super::dialogs::{Outcome, TransactionForm};
use crate::db::{Customer, Kind, TransactionEntry};
use crate::money::format_cents;
use crate::services::Services;
use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;

#[derive(Default)]
pub struct TransactionsView {
    revision: Option<u64>,
    customers: Vec<Customer>,
    entries: Vec<TransactionEntry>,
    form: Option<TransactionForm>,
}

impl View for TransactionsView {
    fn ui(&mut self, ui: &mut egui::Ui, sv: &mut Services) {
        if self.revision != Some(sv.revision) {
            self.customers = sv.db.list_customers("").unwrap_or_default();
            self.entries = sv.db.list_all_transactions(500).unwrap_or_default();
            self.revision = Some(sv.revision);
        }

        ui.horizontal(|ui| {
            ui.heading(format!("{}  Transações", icon::ARROWS_LEFT_RIGHT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(format!("{}  Nova transação", icon::PLUS)).clicked() {
                    self.form = Some(TransactionForm::new(Kind::Debt));
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
                    let (label, glyph, color, sign) = match e.tx.kind {
                        Kind::Debt => ("Dívida", icon::ARROW_UP, Color32::from_rgb(200, 60, 60), "+"),
                        Kind::Payment => ("Pagamento", icon::ARROW_DOWN, Color32::from_rgb(40, 150, 80), "-"),
                    };
                    ui.label(&e.tx.created_at);
                    ui.label(&e.customer_name);
                    ui.label(RichText::new(format!("{glyph} {label}")).color(color));
                    ui.label(RichText::new(format!("{sign} {}", format_cents(e.tx.amount_cents))).color(color));
                    ui.label(&e.tx.note);
                    ui.end_row();
                }
            });
        });

        if let Some(form) = &mut self.form {
            let ctx = ui.ctx().clone();
            if form.show(&ctx, sv, &self.customers) != Outcome::Open {
                self.form = None;
            }
        }
    }
}
