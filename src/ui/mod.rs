//! Cada área do sistema é uma *Strategy* que implementa [`View`];
//! [`create_view`] é a *Simple Factory* que as instancia a partir da [`Section`].

mod backup_view;
mod billing_view;
mod channels_view;
mod customers_view;
mod dialogs;
pub mod lock_view;
pub mod theme;

/// Nome da aplicação exibido na janela, no menu lateral e na tela de senha.
pub const APP_NAME: &str = "Gestor de Crédito";
mod transactions_view;

use crate::services::Services;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Transactions,
    Customers,
    Billing,
    Channels,
    Backup,
}

impl Section {
    pub const ALL: [Section; 5] = [
        Section::Transactions,
        Section::Customers,
        Section::Billing,
        Section::Channels,
        Section::Backup,
    ];

    pub fn icon(self) -> &'static str {
        use egui_phosphor::regular as i;
        match self {
            Section::Transactions => i::ARROWS_LEFT_RIGHT,
            Section::Customers => i::USERS,
            Section::Billing => i::RECEIPT,
            Section::Channels => i::PLUGS_CONNECTED,
            Section::Backup => i::SHIELD_CHECK,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::Transactions => "Transações",
            Section::Customers => "Clientes",
            Section::Billing => "Cobrança automática",
            Section::Channels => "Canais",
            Section::Backup => "Backup e segurança",
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
        Section::Backup => Box::new(backup_view::BackupView::default()),
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
