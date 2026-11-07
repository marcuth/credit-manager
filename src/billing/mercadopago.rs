use super::{BillingError, Checkout, CheckoutRequest, PaymentProvider, PaymentStatus};
use serde_json::{Value, json};

const API: &str = "https://api.mercadopago.com";

/// Checkout Pro: cria uma *preference* e depois consulta os pagamentos pela `external_reference`.
/// Não precisa de webhook (o PC fica atrás do roteador), então usamos consulta periódica.
pub struct MercadoPago {
    token: String,
}

impl MercadoPago {
    pub fn new(token: &str) -> Self {
        Self { token: token.to_string() }
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

fn http(e: ureq::Error) -> BillingError {
    BillingError::Http(e.to_string())
}

impl PaymentProvider for MercadoPago {
    fn name(&self) -> &'static str {
        "Mercado Pago"
    }

    fn create_checkout(&self, req: &CheckoutRequest) -> Result<Checkout, BillingError> {
        let body = json!({
            "items": [{
                "title": req.description,
                "quantity": 1,
                "currency_id": "BRL",
                "unit_price": req.amount_cents as f64 / 100.0,
            }],
            "external_reference": req.external_ref,
        });
        let mut resp = ureq::post(format!("{API}/checkout/preferences"))
            .header("Authorization", self.auth())
            .send_json(&body)
            .map_err(http)?;
        let v: Value = resp.body_mut().read_json().map_err(http)?;
        let url = v["init_point"]
            .as_str()
            .ok_or_else(|| BillingError::Parse("campo init_point ausente".into()))?;
        Ok(Checkout { url: url.to_string() })
    }

    fn payment_status(&self, external_ref: &str) -> Result<PaymentStatus, BillingError> {
        let mut resp = ureq::get(format!("{API}/v1/payments/search"))
            .header("Authorization", self.auth())
            .query("external_reference", external_ref)
            .query("status", "approved")
            .call()
            .map_err(http)?;
        let v: Value = resp.body_mut().read_json().map_err(http)?;
        let results = v["results"]
            .as_array()
            .ok_or_else(|| BillingError::Parse("campo results ausente".into()))?;
        Ok(match results.iter().find(|p| p["status"] == "approved") {
            Some(p) => PaymentStatus::Approved {
                payment_id: p["id"]
                    .as_i64()
                    .map(|i| i.to_string())
                    .or_else(|| p["id"].as_str().map(str::to_string))
                    .ok_or_else(|| BillingError::Parse("pagamento sem id".into()))?,
            },
            None => PaymentStatus::Pending,
        })
    }
}
