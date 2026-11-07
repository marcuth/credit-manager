use super::{BillingError, PaymentProvider, mercadopago::MercadoPago, mock::MockProvider};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    MercadoPago,
    Mock,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 2] = [ProviderKind::MercadoPago, ProviderKind::Mock];

    pub fn key(self) -> &'static str {
        match self {
            ProviderKind::MercadoPago => "mercadopago",
            ProviderKind::Mock => "mock",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::MercadoPago => "Mercado Pago",
            ProviderKind::Mock => "Simulado (apenas para testes)",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|k| k.key() == key).unwrap_or(ProviderKind::MercadoPago)
    }
}

/// Simple Factory: monta o provedor escolhido.
pub fn create_provider(kind: ProviderKind, access_token: &str) -> Result<Arc<dyn PaymentProvider>, BillingError> {
    match kind {
        ProviderKind::MercadoPago => {
            if access_token.trim().is_empty() {
                return Err(BillingError::Config("informe o Access Token do Mercado Pago".into()));
            }
            Ok(Arc::new(MercadoPago::new(access_token.trim())))
        }
        ProviderKind::Mock => Ok(Arc::new(MockProvider)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mercadopago_requires_token() {
        assert!(create_provider(ProviderKind::MercadoPago, "  ").is_err());
        assert!(create_provider(ProviderKind::Mock, "").is_ok());
        assert_eq!(ProviderKind::from_key("mock"), ProviderKind::Mock);
    }
}
