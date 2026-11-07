use super::{View, customer_combo};
use crate::billing::{
    CheckoutRequest,
    channel::{ChannelKind, ChannelStatus, MessageChannel, charge_message},
    factory::ProviderKind,
};
use crate::db::{Charge, Customer};
use crate::money::{format_cents, parse_cents};
use crate::services::{Event, Services};
use eframe::egui::{self, RichText};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct BillingView {
    revision: Option<u64>,
    customers: Vec<Customer>,
    charges: Vec<Charge>,
    // configuração
    loaded_settings: bool,
    provider_kind: ProviderKind,
    token: String,
    // nova cobrança
    customer: Option<i64>,
    amount: String,
}

impl Default for BillingView {
    fn default() -> Self {
        Self {
            revision: None,
            customers: vec![],
            charges: vec![],
            loaded_settings: false,
            provider_kind: ProviderKind::MercadoPago,
            token: String::new(),
            customer: None,
            amount: String::new(),
        }
    }
}

impl View for BillingView {
    fn ui(&mut self, ui: &mut egui::Ui, sv: &mut Services) {
        if !self.loaded_settings {
            self.provider_kind = ProviderKind::from_key(&sv.db.get_setting("provider").ok().flatten().unwrap_or_default());
            self.token = sv.db.get_setting("mp_access_token").ok().flatten().unwrap_or_default();
            self.loaded_settings = true;
        }
        if self.revision != Some(sv.revision) {
            self.customers = sv.db.list_customers("").unwrap_or_default();
            self.charges = sv.db.list_charges().unwrap_or_default();
            self.revision = Some(sv.revision);
        }

        ui.heading("Cobrança automática");
        ui.add_space(8.0);

        egui::CollapsingHeader::new("Configuração do provedor")
            .default_open(sv.provider.is_err())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Provedor:");
                    egui::ComboBox::from_id_salt("provider")
                        .selected_text(self.provider_kind.label())
                        .show_ui(ui, |ui| {
                            for k in ProviderKind::ALL {
                                ui.selectable_value(&mut self.provider_kind, k, k.label());
                            }
                        });
                });
                if self.provider_kind == ProviderKind::MercadoPago {
                    ui.horizontal(|ui| {
                        ui.label("Access Token:");
                        ui.add(egui::TextEdit::singleline(&mut self.token).password(true).desired_width(360.0));
                    });
                }
                if ui.button("Salvar configuração").clicked() {
                    let r1 = sv.db.set_setting("provider", self.provider_kind.key());
                    let r2 = sv.db.set_setting("mp_access_token", self.token.trim());
                    sv.reload_provider();
                    match (r1, r2, &sv.provider) {
                        (Ok(()), Ok(()), Ok(p)) => sv.notify(format!("Provedor ativo: {}", p.name())),
                        (_, _, Err(e)) => sv.notify(e.to_string()),
                        (Err(e), _, _) | (_, Err(e), _) => sv.notify(e.to_string()),
                    }
                }
            });
        ui.add_space(8.0);

        ui.group(|ui| {
            ui.label(RichText::new("Nova cobrança").strong());
            ui.horizontal_wrapped(|ui| {
                let before = self.customer;
                customer_combo(ui, "bill_customer", &self.customers, &mut self.customer);
                if before != self.customer
                    && let Some(c) = self.customers.iter().find(|c| Some(c.id) == self.customer)
                    && c.balance_cents > 0
                {
                    self.amount = format_cents(c.balance_cents).replace("R$ ", "").replace('.', "");
                }
                ui.add(egui::TextEdit::singleline(&mut self.amount).hint_text("Valor").desired_width(120.0));
                let ready = sv.provider.is_ok();
                if ui.add_enabled(ready, egui::Button::new("Gerar link de pagamento")).clicked() {
                    match (self.customer, parse_cents(&self.amount).filter(|c| *c > 0), sv.provider.clone()) {
                        (None, ..) => sv.notify("Escolha um cliente."),
                        (_, None, _) => sv.notify("Valor inválido."),
                        (_, _, Err(e)) => sv.notify(e.to_string()),
                        (Some(customer_id), Some(cents), Ok(provider)) => {
                            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
                            let external_ref = format!("fiado-{customer_id}-{nanos}");
                            let req = CheckoutRequest {
                                amount_cents: cents,
                                description: "Pagamento de fiado".into(),
                                external_ref: external_ref.clone(),
                            };
                            let tx = sv.events.clone();
                            std::thread::spawn(move || {
                                let result = provider.create_checkout(&req);
                                let _ = tx.send(Event::CheckoutCreated {
                                    customer_id,
                                    amount_cents: cents,
                                    external_ref,
                                    result,
                                });
                            });
                            sv.notify("Gerando link...");
                        }
                    }
                }
                if !ready {
                    ui.label("Configure o provedor acima.");
                }
            });
        });
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new("Cobranças").strong());
            if ui.add_enabled(sv.provider.is_ok(), egui::Button::new("Verificar pagamentos agora")).clicked() {
                sv.check_pending();
                sv.notify("Verificando pagamentos...");
            }
            ui.label("(o sistema também verifica sozinho a cada 30 segundos)");
        });
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for ch in &self.charges {
                ui.horizontal(|ui| {
                    ui.label(&ch.created_at);
                    ui.label(&ch.customer_name);
                    ui.label(format_cents(ch.amount_cents));
                    if ch.paid {
                        ui.colored_label(egui::Color32::from_rgb(40, 150, 80), "Pago");
                        return;
                    }
                    ui.colored_label(egui::Color32::from_rgb(200, 140, 0), "Aguardando");
                    if ui.button("Abrir link").clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&ch.url));
                    }
                    if ui.button("Copiar link").clicked() {
                        ui.ctx().copy_text(ch.url.clone());
                        sv.notify("Link copiado.");
                    }
                    if let Some(customer) = self.customers.iter().find(|c| c.id == ch.customer_id) {
                        for kind in ChannelKind::ALL {
                            let channel = sv.channels.get(kind);
                            let ready = channel.status() == ChannelStatus::Ready;
                            let reachable = channel.can_reach(customer);
                            let hint = if !ready {
                                "Conecte o canal na tela Canais"
                            } else {
                                "Cliente sem esse contato cadastrado"
                            };
                            let resp = ui
                                .add_enabled(ready && reachable, egui::Button::new(format!("Enviar por {}", kind.label())))
                                .on_disabled_hover_text(hint);
                            if resp.clicked() {
                                let text = charge_message(&customer.name, ch.amount_cents, &ch.url);
                                sv.send_via(kind, customer.clone(), text);
                                sv.notify(format!("Enviando por {}...", kind.label()));
                            }
                        }
                    }
                });
            }
        });
    }
}
