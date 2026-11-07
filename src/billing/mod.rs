//! Cobrança automática.
//!
//! * [`PaymentProvider`] é a *Strategy* de provedor de pagamento.
//! * [`factory::create_provider`] é a *Simple Factory* que escolhe a estratégia.
//! * [`channel::MessageChannel`] é a *Strategy* de canal de envio da mensagem.

pub mod channel;
pub mod factory;
pub mod mercadopago;
pub mod mock;

#[derive(Debug, Clone, thiserror::Error)]
pub enum BillingError {
    #[error("configuração incompleta: {0}")]
    Config(String),
    #[error("falha na comunicação com o provedor: {0}")]
    Http(String),
    #[error("resposta inesperada do provedor: {0}")]
    Parse(String),
}

pub struct CheckoutRequest {
    pub amount_cents: i64,
    pub description: String,
    /// Identificador nosso, devolvido pelo provedor para casar o pagamento.
    pub external_ref: String,
}

#[derive(Debug, Clone)]
pub struct Checkout {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaymentStatus {
    Pending,
    Approved { payment_id: String },
}

pub trait PaymentProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn create_checkout(&self, req: &CheckoutRequest) -> Result<Checkout, BillingError>;
    fn payment_status(&self, external_ref: &str) -> Result<PaymentStatus, BillingError>;
}
