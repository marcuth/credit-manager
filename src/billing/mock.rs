use super::{BillingError, Checkout, CheckoutRequest, PaymentProvider, PaymentStatus};

/// Provedor falso: aprova qualquer cobrança na primeira consulta. Só para testar o fluxo.
pub struct MockProvider;

impl PaymentProvider for MockProvider {
    fn name(&self) -> &'static str {
        "Simulado"
    }

    fn create_checkout(&self, req: &CheckoutRequest) -> Result<Checkout, BillingError> {
        Ok(Checkout { url: format!("https://example.invalid/checkout/{}", req.external_ref) })
    }

    fn payment_status(&self, external_ref: &str) -> Result<PaymentStatus, BillingError> {
        Ok(PaymentStatus::Approved { payment_id: format!("mock-{external_ref}") })
    }
}
