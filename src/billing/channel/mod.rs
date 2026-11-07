//! Canais de envio da cobrança (Strategy): cada canal sabe dizer se está pronto,
//! se alcança um cliente e enviar a mensagem de verdade.

mod telegram;
mod whatsapp;

use crate::db::Customer;
use crate::money::format_cents;
use std::{path::Path, sync::Arc};

pub use telegram::TelegramChannel;
pub use whatsapp::WhatsAppChannel;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelStatus {
    /// Falta configurar (ex.: token do bot).
    NotConfigured,
    Disconnected,
    Connecting,
    /// WhatsApp aguardando o aparelho ler este QR code.
    WaitingQr(String),
    Ready,
    Error(String),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ChannelError {
    #[error("o canal não está conectado")]
    NotReady,
    #[error("o cliente não tem contato cadastrado para este canal")]
    NoContact,
    #[error("falha no envio: {0}")]
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    WhatsApp,
    Telegram,
}

impl ChannelKind {
    pub const ALL: [ChannelKind; 2] = [ChannelKind::WhatsApp, ChannelKind::Telegram];

    pub fn label(self) -> &'static str {
        match self {
            ChannelKind::WhatsApp => "WhatsApp",
            ChannelKind::Telegram => "Telegram",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ChannelKind::WhatsApp => "whatsapp",
            ChannelKind::Telegram => "telegram",
        }
    }

    /// Chave da configuração "enviar automaticamente ao gerar a cobrança".
    pub fn auto_send_setting(self) -> String {
        format!("channel_{}_auto", self.key())
    }
}

pub trait MessageChannel: Send + Sync {
    fn kind(&self) -> ChannelKind;
    fn status(&self) -> ChannelStatus;
    fn can_reach(&self, customer: &Customer) -> bool;
    /// Bloqueante: chame de uma thread de trabalho, nunca da thread da interface.
    fn send(&self, customer: &Customer, text: &str) -> Result<(), ChannelError>;
}

/// Os canais do sistema. Mantemos os tipos concretos porque as telas de
/// configuração precisam de operações específicas de cada canal.
pub struct Channels {
    pub whatsapp: Arc<WhatsAppChannel>,
    pub telegram: Arc<TelegramChannel>,
}

impl Channels {
    pub fn new(data_dir: &Path) -> std::io::Result<Self> {
        Ok(Self {
            whatsapp: Arc::new(WhatsAppChannel::new(data_dir.join("whatsapp.db"))?),
            telegram: Arc::new(TelegramChannel::new()),
        })
    }

    /// Simple Factory: devolve o canal como estratégia.
    pub fn get(&self, kind: ChannelKind) -> Arc<dyn MessageChannel> {
        match kind {
            ChannelKind::WhatsApp => self.whatsapp.clone(),
            ChannelKind::Telegram => self.telegram.clone(),
        }
    }
}

pub fn charge_message(customer_name: &str, cents: i64, url: &str) -> String {
    format!("Olá, {customer_name}! Você tem {} em aberto. Pague por este link: {url}", format_cents(cents))
}

/// Número no formato do WhatsApp (DDI 55 + DDD + número), só dígitos.
pub(crate) fn whatsapp_number(phone: &str) -> Option<String> {
    let digits: String = phone.chars().filter(char::is_ascii_digit).collect();
    match digits.len() {
        10 | 11 => Some(format!("55{digits}")),
        12 | 13 if digits.starts_with("55") => Some(digits),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_normalization() {
        assert_eq!(whatsapp_number("(11) 91234-5678").as_deref(), Some("5511912345678"));
        assert_eq!(whatsapp_number("+55 11 91234-5678").as_deref(), Some("5511912345678"));
        assert_eq!(whatsapp_number("123"), None);
    }

    #[test]
    fn message_mentions_amount_and_link() {
        let m = charge_message("Ana", 2550, "http://x");
        assert!(m.contains("R$ 25,50") && m.contains("http://x"));
    }
}
