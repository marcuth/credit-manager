//! Diálogos modais reutilizáveis e os formulários de cadastro compartilhados pelas telas.

use super::customer_combo;
use super::theme::{self, Btn};
use crate::db::{Customer, Kind};
use crate::money::parse_cents;
use crate::services::Services;
use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;

pub enum DialogAction {
    None,
    Submit,
    Cancel,
}

/// Resultado de mostrar um formulário num frame.
#[derive(PartialEq, Eq)]
pub enum Outcome {
    Open,
    Cancelled,
    Saved,
}

/// Moldura padrão: título, corpo, erro (se houver) e botões Salvar/Cancelar.
/// Esc ou clique fora cancelam; Enter envia.
pub fn form_dialog(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    submit_label: &str,
    error: Option<&str>,
    body: impl FnOnce(&mut egui::Ui),
) -> DialogAction {
    let resp = egui::Modal::new(egui::Id::unique(id))
        .frame(theme::modal_frame())
        .backdrop_color(theme::SCRIM)
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(theme::title(title));
            ui.add_space(8.0);
            body(ui);
            if let Some(e) = error {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("{}  {e}", icon::WARNING)).color(theme::DANGER));
            }
            ui.add_space(10.0);
            let mut action = DialogAction::None;
            ui.horizontal(|ui| {
                if ui.add(Btn::primary(format!("{}  {submit_label}", icon::CHECK))).clicked() {
                    action = DialogAction::Submit;
                }
                if ui.add(Btn::secondary(format!("{}  Cancelar", icon::X))).clicked() {
                    action = DialogAction::Cancel;
                }
            });
            if matches!(action, DialogAction::None) && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                action = DialogAction::Submit;
            }
            action
        });
    match resp.inner {
        DialogAction::None if resp.should_close() => DialogAction::Cancel,
        other => other,
    }
}

/// Campo de texto com rótulo, ocupando toda a largura do diálogo.
pub fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, password: bool) -> egui::Response {
    ui.label(theme::dim(label));
    let r = ui.add(
        egui::TextEdit::singleline(value).hint_text(hint).password(password).desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);
    r
}

/// Campo para segredos (tokens) com botão de mostrar/ocultar. Quando `reveal` é `false`
/// o texto fica mascarado.
pub fn secret_field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, reveal: &mut bool) {
    ui.label(theme::dim(label));
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 64.0).max(120.0);
        ui.add(egui::TextEdit::singleline(value).hint_text(hint).password(!*reveal).desired_width(width));
        let (glyph, tip) = if *reveal { (icon::EYE_SLASH, "Ocultar") } else { (icon::EYE, "Mostrar") };
        if ui.add(Btn::secondary(glyph).min_width(40.0)).on_hover_text(tip).clicked() {
            *reveal = !*reveal;
        }
    });
    ui.add_space(4.0);
}

// ---------------------------------------------------------------- novo cliente

#[derive(Default)]
pub struct CustomerForm {
    name: String,
    phone: String,
    email: String,
    error: Option<String>,
    /// Id do cliente criado, quando o formulário foi salvo.
    pub created_id: Option<i64>,
}

impl CustomerForm {
    pub fn show(&mut self, ctx: &egui::Context, sv: &mut Services) -> Outcome {
        let action = form_dialog(ctx, "customer_form", &format!("{}  Novo cliente", icon::USER_PLUS), "Cadastrar", self.error.as_deref(), |ui| {
            field(ui, "Nome", &mut self.name, "Nome do cliente", false);
            field(ui, "Celular com DDD (opcional)", &mut self.phone, "(11) 91234-5678", false);
            field(ui, "E-mail (opcional)", &mut self.email, "cliente@exemplo.com", false);
        });
        match action {
            DialogAction::None => Outcome::Open,
            DialogAction::Cancel => Outcome::Cancelled,
            DialogAction::Submit => {
                if self.name.trim().is_empty() {
                    self.error = Some("Informe o nome do cliente.".into());
                    return Outcome::Open;
                }
                match sv.db.add_customer(&self.name, &self.phone, &self.email) {
                    Ok(id) => {
                        self.created_id = Some(id);
                        sv.changed();
                        sv.notify("Cliente cadastrado.");
                        Outcome::Saved
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        Outcome::Open
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- transação

pub struct TransactionForm {
    pub customer: Option<i64>,
    /// `true`: mostra a caixa de seleção de cliente; `false`: o cliente já está definido.
    choose_customer: bool,
    kind: Kind,
    amount: String,
    note: String,
    error: Option<String>,
}

impl TransactionForm {
    /// Transação para qualquer cliente (tela Transações).
    pub fn new(kind: Kind) -> Self {
        Self { customer: None, choose_customer: true, kind, amount: String::new(), note: String::new(), error: None }
    }

    /// Transação para um cliente específico (tela Clientes).
    pub fn for_customer(customer_id: i64, kind: Kind) -> Self {
        Self { customer: Some(customer_id), choose_customer: false, ..Self::new(kind) }
    }

    pub fn show(&mut self, ctx: &egui::Context, sv: &mut Services, customers: &[Customer]) -> Outcome {
        let title = match (self.choose_customer, self.kind) {
            (true, _) => format!("{}  Nova transação", icon::ARROWS_LEFT_RIGHT),
            (false, Kind::Debt) => format!("{}  Adicionar dívida", icon::ARROW_UP),
            (false, Kind::Payment) => format!("{}  Adicionar pagamento", icon::ARROW_DOWN),
        };
        let action = form_dialog(ctx, "transaction_form", &title, "Salvar", self.error.as_deref(), |ui| {
            if self.choose_customer {
                ui.label(theme::dim("Cliente"));
                customer_combo(ui, "tx_form_customer", customers, &mut self.customer);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if theme::pill_tab(ui, self.kind == Kind::Debt, format!("{}  Dívida", icon::ARROW_UP)).clicked() {
                        self.kind = Kind::Debt;
                    }
                    if theme::pill_tab(ui, self.kind == Kind::Payment, format!("{}  Pagamento", icon::ARROW_DOWN)).clicked() {
                        self.kind = Kind::Payment;
                    }
                });
                ui.add_space(6.0);
            } else if let Some(c) = customers.iter().find(|c| Some(c.id) == self.customer) {
                ui.label(RichText::new(&c.name).strong());
                ui.add_space(4.0);
            }
            field(ui, "Valor", &mut self.amount, "Ex: 25,50", false);
            field(ui, "Anotação (opcional)", &mut self.note, "Ex: compras do mês", false);
        });
        match action {
            DialogAction::None => Outcome::Open,
            DialogAction::Cancel => Outcome::Cancelled,
            DialogAction::Submit => {
                let Some(customer) = self.customer else {
                    self.error = Some("Escolha um cliente.".into());
                    return Outcome::Open;
                };
                let Some(cents) = parse_cents(&self.amount).filter(|c| *c > 0) else {
                    self.error = Some("Valor inválido. Exemplo: 25,50".into());
                    return Outcome::Open;
                };
                match sv.db.add_transaction(customer, self.kind, cents, &self.note) {
                    Ok(()) => {
                        sv.changed();
                        sv.notify("Transação registrada.");
                        Outcome::Saved
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        Outcome::Open
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Simula clique no campo e digitação, sem janela, e devolve o texto resultante.
    fn type_into_field(password: bool, modal: bool) -> String {
        let ctx = egui::Context::default();
        let rect = Cell::new(egui::Rect::NOTHING);
        let mut value = String::new();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let mut frame = |events: Vec<egui::Event>, i: usize| {
            let raw = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(i as f64 * 0.016),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                if modal {
                    let ctx = ui.ctx().clone();
                    form_dialog(&ctx, "t", "Titulo", "OK", None, |ui| {
                        rect.set(field(ui, "Rotulo", &mut value, "dica", password).rect);
                    });
                } else {
                    rect.set(field(ui, "Rotulo", &mut value, "dica", password).rect);
                }
            });
            out.textures_delta.clear();
        };
        for i in 0..4 {
            frame(vec![], i); // aquecimento: o Modal só se posiciona após alguns frames
        }
        let pos = rect.get().center();
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(pos)], 5);
        frame(vec![button(true)], 6);
        frame(vec![button(false)], 7);
        frame(vec![egui::Event::Text("123456:ABC-def".into())], 8);
        frame(vec![], 9);
        value
    }

    #[test]
    fn control_field_outside_modal_accepts_typing() {
        assert_eq!(type_into_field(false, false), "123456:ABC-def");
    }

    #[test]
    fn plain_field_in_modal_accepts_typing() {
        assert_eq!(type_into_field(false, true), "123456:ABC-def");
    }

    #[test]
    fn password_field_in_modal_accepts_typing() {
        assert_eq!(type_into_field(true, true), "123456:ABC-def");
    }
}
