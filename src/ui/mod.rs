//! Cada área do sistema é uma *Strategy* que implementa [`View`];
//! [`create_view`] é a *Simple Factory* que as instancia a partir da [`Section`].

mod billing_view;
mod channels_view;
mod customers_view;
mod transactions_view;

use crate::services::Services;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Transactions,
    Customers,
    Billing,
    Channels,
}

impl Section {
    pub const ALL: [Section; 4] = [Section::Transactions, Section::Customers, Section::Billing, Section::Channels];

    pub fn label(self) -> &'static str {
        match self {
            Section::Transactions => "Transações",
            Section::Customers => "Clientes",
            Section::Billing => "Cobrança automática",
            Section::Channels => "Canais",
        }
    }
}

pub trait View {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, services: &mut Services);
}

pub fn create_view(section: Section) -> Box<dyn View> {
    match section {
        Section::Transactions => Box::new(transactions_view::TransactionsView::default()),
        Section::Customers => Box::new(customers_view::CustomersView::default()),
        Section::Billing => Box::new(billing_view::BillingView::default()),
        Section::Channels => Box::new(channels_view::ChannelsView::default()),
    }
}

/// Caixa de seleção de cliente, compartilhada pelas telas.
pub fn customer_combo(
    ui: &mut eframe::egui::Ui,
    id: &str,
    customers: &[crate::db::Customer],
    selected: &mut Option<i64>,
) {
    let current = customers
        .iter()
        .find(|c| Some(c.id) == *selected)
        .map(|c| c.name.as_str())
        .unwrap_or("Escolha um cliente");
    eframe::egui::ComboBox::from_id_salt(id).selected_text(current).width(240.0).show_ui(ui, |ui| {
        for c in customers {
            ui.selectable_value(selected, Some(c.id), &c.name);
        }
    });
}
