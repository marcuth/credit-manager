use super::dialogs::{DialogAction, Outcome, field, form_dialog, secret_field};
use super::theme::{self, Btn, Tone};
use super::{View, customer_combo};
use crate::billing::{
    CheckoutRequest,
    channel::{ChannelKind, ChannelStatus, charge_message},
    factory::ProviderKind,
};
use crate::db::{Charge, Customer};
use crate::money::{format_cents, format_plain, parse_cents};
use crate::services::{Event, Services};
use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct BillingView {
    revision: Option<u64>,
    customers: Vec<Customer>,
    charges: Vec<Charge>,
    charge_form: Option<ChargeForm>,
    provider_form: Option<ProviderForm>,
}

// ------------------------------------------------------------ nova cobrança

struct ChargeForm {
    customer: Option<i64>,
    amount: String,
    error: Option<String>,
}

impl ChargeForm {
    fn show(&mut self, ctx: &egui::Context, sv: &mut Services, customers: &[Customer]) -> Outcome {
        let action = form_dialog(
            ctx,
            "charge_form",
            &format!("{}  Nova cobrança", icon::RECEIPT),
            "Gerar link",
            self.error.as_deref(),
            |ui| {
                ui.label("Cliente");
                let before = self.customer;
                customer_combo(ui, "charge_customer", customers, &mut self.customer);
                // Ao escolher um cliente que deve, sugere o saldo como valor.
                if before != self.customer
                    && let Some(c) = customers.iter().find(|c| Some(c.id) == self.customer)
                    && c.balance_cents > 0
                {
                    self.amount = format_plain(c.balance_cents);
                }
                ui.add_space(6.0);
                field(ui, "Valor", &mut self.amount, "Ex: 25,50", false);
            },
        );
        match action {
            DialogAction::None => Outcome::Open,
            DialogAction::Cancel => Outcome::Cancelled,
            DialogAction::Submit => {
                let Some(customer_id) = self.customer else {
                    self.error = Some("Escolha um cliente.".into());
                    return Outcome::Open;
                };
                let Some(cents) = parse_cents(&self.amount).filter(|c| *c > 0) else {
                    self.error = Some("Valor inválido. Exemplo: 25,50".into());
                    return Outcome::Open;
                };
                let provider = match sv.provider.clone() {
                    Ok(p) => p,
                    Err(e) => {
                        self.error = Some(e.to_string());
                        return Outcome::Open;
                    }
                };
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
                    let _ = tx.send(Event::CheckoutCreated { customer_id, amount_cents: cents, external_ref, result });
                });
                sv.notify("Gerando link...");
                Outcome::Saved
            }
        }
    }
}

// ------------------------------------------------------------ provedor

struct ProviderForm {
    kind: ProviderKind,
    token: String,
    reveal: bool,
    error: Option<String>,
}

impl ProviderForm {
    fn load(sv: &Services) -> Self {
        Self {
            kind: ProviderKind::from_key(&sv.db.get_setting("provider").ok().flatten().unwrap_or_default()),
            token: sv.secret("mp_access_token"),
            reveal: true,
            error: None,
        }
    }

    fn show(&mut self, ctx: &egui::Context, sv: &mut Services) -> Outcome {
        let action = form_dialog(
            ctx,
            "provider_form",
            &format!("{}  Provedor de pagamento", icon::GEAR),
            "Salvar",
            self.error.as_deref(),
            |ui| {
                ui.label("Provedor");
                egui::ComboBox::from_id_salt("provider_kind").selected_text(self.kind.label()).show_ui(ui, |ui| {
                    for k in ProviderKind::ALL {
                        ui.selectable_value(&mut self.kind, k, k.label());
                    }
                });
                ui.add_space(6.0);
                if self.kind == ProviderKind::MercadoPago {
                    secret_field(ui, "Access Token", &mut self.token, "APP_USR-...", &mut self.reveal);
                }
            },
        );
        match action {
            DialogAction::None => Outcome::Open,
            DialogAction::Cancel => Outcome::Cancelled,
            DialogAction::Submit => {
                let r1 = sv.db.set_setting("provider", self.kind.key());
                let r2 = sv.set_secret("mp_access_token", self.token.trim());
                sv.reload_provider();
                match (r1, r2, &sv.provider) {
                    (Err(e), ..) | (_, Err(e), _) => self.error = Some(e.to_string()),
                    (_, _, Err(e)) => self.error = Some(e.to_string()),
                    (Ok(()), Ok(()), Ok(p)) => {
                        let msg = format!("Provedor ativo: {}", p.name());
                        sv.notify(msg);
                        return Outcome::Saved;
                    }
                }
                Outcome::Open
            }
        }
    }
}

// ------------------------------------------------------------ tela

impl View for BillingView {
    fn ui(&mut self, ui: &mut egui::Ui, sv: &mut Services) {
        if self.revision != Some(sv.revision) {
            self.customers = sv.db.list_customers("").unwrap_or_default();
            self.charges = sv.db.list_charges().unwrap_or_default();
            self.revision = Some(sv.revision);
        }

        ui.horizontal(|ui| {
            ui.heading(format!("{}  Cobrança automática", icon::RECEIPT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(Btn::secondary(format!("{}  Provedor", icon::GEAR))).clicked() {
                    self.provider_form = Some(ProviderForm::load(sv));
                }
                let ready = sv.provider.is_ok();
                if ui.add_enabled(ready, Btn::primary(format!("{}  Nova cobrança", icon::PLUS))).clicked() {
                    self.charge_form = Some(ChargeForm { customer: None, amount: String::new(), error: None });
                }
            });
        });
        if let Err(e) = &sv.provider {
            ui.label(RichText::new(format!("{}  {e}. Use o botão Provedor.", icon::WARNING)).color(theme::DANGER));
        }
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new("Cobranças").strong());
            if ui
                .add_enabled(sv.provider.is_ok(), Btn::secondary(format!("{}  Verificar pagamentos agora", icon::ARROWS_CLOCKWISE)))
                .clicked()
            {
                sv.check_pending();
                sv.notify("Verificando pagamentos...");
            }
            ui.label("(o sistema também verifica sozinho a cada 30 segundos)");
        });
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for ch in &self.charges {
                ui.horizontal_wrapped(|ui| {
                    ui.label(&ch.created_at);
                    ui.label(format!("{}  {}", icon::USER, ch.customer_name));
                    ui.label(format_cents(ch.amount_cents));
                    if ch.paid {
                        theme::badge(ui, format!("{}  Pago", icon::CHECK_CIRCLE), Tone::Success);
                        return;
                    }
                    theme::badge(ui, format!("{}  Aguardando", icon::CLOCK), Tone::Warning);
                    if ui.add(Btn::secondary(format!("{}  Abrir", icon::ARROW_SQUARE_OUT)).compact()).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&ch.url));
                    }
                    if ui.add(Btn::secondary(format!("{}  Copiar link", icon::COPY)).compact()).clicked() {
                        ui.ctx().copy_text(ch.url.clone());
                        sv.notify("Link copiado.");
                    }
                    if let Some(customer) = self.customers.iter().find(|c| c.id == ch.customer_id) {
                        for kind in ChannelKind::ALL {
                            let channel = sv.channels.get(kind);
                            let ready = channel.status() == ChannelStatus::Ready;
                            let hint = if !ready { "Conecte o canal na tela Canais" } else { "Cliente sem esse contato cadastrado" };
                            let glyph = match kind {
                                ChannelKind::WhatsApp => icon::WHATSAPP_LOGO,
                                ChannelKind::Telegram => icon::TELEGRAM_LOGO,
                            };
                            let resp = ui
                                .add_enabled(ready && channel.can_reach(customer), Btn::secondary(format!("{glyph}  Enviar por {}", kind.label())).compact())
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

        let ctx = ui.ctx().clone();
        if let Some(form) = &mut self.charge_form
            && form.show(&ctx, sv, &self.customers) != Outcome::Open
        {
            self.charge_form = None;
        }
        if let Some(form) = &mut self.provider_form
            && form.show(&ctx, sv) != Outcome::Open
        {
            self.provider_form = None;
        }
    }
}
