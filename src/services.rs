use crate::billing::{
    BillingError, Checkout, PaymentProvider, PaymentStatus,
    factory::{ProviderKind, create_provider},
};
use crate::billing::channel::{ChannelError, ChannelKind, Channels};
use crate::db::{Customer, Db};
use std::{
    path::Path,
    sync::{Arc, mpsc::Sender},
};

/// Resultados de tarefas de rede, que rodam em threads para não travar a janela.
pub enum Event {
    CheckoutCreated {
        customer_id: i64,
        amount_cents: i64,
        external_ref: String,
        result: Result<Checkout, BillingError>,
    },
    PaymentChecked {
        charge_id: i64,
        result: Result<PaymentStatus, BillingError>,
    },
    MessageSent {
        channel: ChannelKind,
        customer: String,
        result: Result<(), ChannelError>,
    },
    /// O cliente abriu o link do bot no Telegram e enviou `/start <codigo>`.
    TelegramLinked {
        code: String,
        chat_id: i64,
    },
}

/// Tudo o que as telas compartilham.
pub struct Services {
    pub db: Db,
    pub provider: Result<Arc<dyn PaymentProvider>, BillingError>,
    pub events: Sender<Event>,
    pub channels: Channels,
    /// Aumenta a cada mudança nos dados; as telas recarregam quando muda.
    pub revision: u64,
    pub status: Option<String>,
}

impl Services {
    pub fn new(db: Db, events: Sender<Event>, data_dir: &Path) -> std::io::Result<Self> {
        let mut s = Self {
            db,
            provider: Err(BillingError::Config("não carregado".into())),
            events,
            channels: Channels::new(data_dir)?,
            revision: 0,
            status: None,
        };
        s.reload_provider();
        // Reativa os canais que já estavam configurados.
        if let Some(token) = s.db.get_setting("telegram_token").ok().flatten() {
            s.channels.telegram.configure(&token, s.events.clone());
        }
        if s.db.get_setting("whatsapp_enabled").ok().flatten().as_deref() == Some("1") {
            s.channels.whatsapp.connect();
        }
        Ok(s)
    }

    pub fn auto_send_enabled(&self, kind: ChannelKind) -> bool {
        self.db.get_setting(&kind.auto_send_setting()).ok().flatten().as_deref() == Some("1")
    }

    /// Envia a mensagem em segundo plano; o resultado volta como [`Event::MessageSent`].
    pub fn send_via(&self, kind: ChannelKind, customer: Customer, text: String) {
        let channel = self.channels.get(kind);
        let tx = self.events.clone();
        std::thread::spawn(move || {
            let result = channel.send(&customer, &text);
            let _ = tx.send(Event::MessageSent { channel: kind, customer: customer.name, result });
        });
    }

    pub fn changed(&mut self) {
        self.revision += 1;
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.status = Some(msg.into());
    }

    pub fn reload_provider(&mut self) {
        let kind = ProviderKind::from_key(&self.db.get_setting("provider").ok().flatten().unwrap_or_default());
        let token = self.db.get_setting("mp_access_token").ok().flatten().unwrap_or_default();
        self.provider = create_provider(kind, &token);
    }

    /// Consulta o provedor sobre todas as cobranças pendentes (em segundo plano).
    pub fn check_pending(&mut self) {
        let Ok(provider) = self.provider.clone() else { return };
        let Ok(pending) = self.db.pending_charges() else { return };
        if pending.is_empty() {
            return;
        }
        let tx = self.events.clone();
        std::thread::spawn(move || {
            for (charge_id, external_ref) in pending {
                let result = provider.payment_status(&external_ref);
                if tx.send(Event::PaymentChecked { charge_id, result }).is_err() {
                    return;
                }
            }
        });
    }
}
