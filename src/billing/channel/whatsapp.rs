use super::{ChannelError, ChannelKind, ChannelStatus, MessageChannel, whatsapp_number};
use crate::db::Customer;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use whatsapp_rust::prelude::*;

#[derive(Default)]
struct Shared {
    status: Option<ChannelStatus>,
    client: Option<Arc<Client>>,
}

/// WhatsApp pela biblioteca não oficial `whatsapp-rust` (protocolo do WhatsApp Web).
/// Roda num runtime tokio próprio; a interface só lê o status e chama `send` de uma thread de trabalho.
pub struct WhatsAppChannel {
    runtime: Option<tokio::runtime::Runtime>,
    db_path: PathBuf,
    shared: Arc<Mutex<Shared>>,
    handle: Arc<Mutex<Option<BotHandle>>>,
}

impl WhatsAppChannel {
    pub fn new(db_path: PathBuf) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        Ok(Self {
            runtime: Some(runtime),
            db_path,
            shared: Arc::new(Mutex::new(Shared::default())),
            handle: Arc::new(Mutex::new(None)),
        })
    }

    fn rt(&self) -> &tokio::runtime::Runtime {
        self.runtime.as_ref().expect("runtime vivo até o Drop")
    }

    fn set_status(shared: &Mutex<Shared>, status: ChannelStatus) {
        shared.lock().unwrap().status = Some(status);
    }

    /// Existe uma sessão salva (aparelho já pareado)?
    pub fn has_session(&self) -> bool {
        self.db_path.exists()
    }

    /// Conecta; se não houver sessão salva, passa por `WaitingQr` até o aparelho ler o código.
    pub fn connect(&self) {
        {
            let mut g = self.shared.lock().unwrap();
            if matches!(
                g.status,
                Some(ChannelStatus::Connecting | ChannelStatus::WaitingQr(_) | ChannelStatus::Ready)
            ) {
                return; // já conectado ou conectando
            }
            g.status = Some(ChannelStatus::Connecting);
        }
        let (shared, handle, path) = (self.shared.clone(), self.handle.clone(), self.db_path.clone());
        self.rt().spawn(async move {
            let (s_qr, s_ok, s_out) = (shared.clone(), shared.clone(), shared.clone());
            let built = async {
                let store = SqliteStore::new(path.to_string_lossy().as_ref()).await?;
                Bot::builder()
                    .with_backend(store)
                    .on_qr_code(move |code, _timeout| {
                        let s = s_qr.clone();
                        async move { Self::set_status(&s, ChannelStatus::WaitingQr(code)) }
                    })
                    .on_connected(move |client| {
                        let s = s_ok.clone();
                        async move {
                            let mut g = s.lock().unwrap();
                            g.client = Some(client);
                            g.status = Some(ChannelStatus::Ready);
                        }
                    })
                    .on_logged_out(move |_| {
                        let s = s_out.clone();
                        async move {
                            let mut g = s.lock().unwrap();
                            g.client = None;
                            g.status = Some(ChannelStatus::Error(
                                "o aparelho foi desvinculado; use \"Desvincular\" e conecte de novo".into(),
                            ));
                        }
                    })
                    .build()
                    .await
            }
            .await;
            match built {
                Ok(bot) => *handle.lock().unwrap() = Some(bot.spawn()),
                Err(e) => Self::set_status(&shared, ChannelStatus::Error(e.to_string())),
            }
        });
    }

    /// Desliga a conexão mantendo a sessão salva.
    pub fn disconnect(&self) {
        let handle = self.handle.lock().unwrap().take();
        let shared = self.shared.clone();
        self.rt().spawn(async move {
            if let Some(h) = handle {
                h.shutdown().await;
            }
            let mut g = shared.lock().unwrap();
            g.client = None;
            g.status = Some(ChannelStatus::Disconnected);
        });
    }

    /// Desliga e apaga a sessão salva: será preciso ler um novo QR code.
    pub fn reset_session(&self) {
        let handle = self.handle.lock().unwrap().take();
        let shared = self.shared.clone();
        let path = self.db_path.clone();
        self.rt().spawn(async move {
            if let Some(h) = handle {
                h.shutdown().await;
            }
            for suffix in ["", "-wal", "-shm"] {
                let mut p = path.clone().into_os_string();
                p.push(suffix);
                let _ = std::fs::remove_file(p);
            }
            let mut g = shared.lock().unwrap();
            g.client = None;
            g.status = Some(ChannelStatus::Disconnected);
        });
    }
}

impl Drop for WhatsAppChannel {
    fn drop(&mut self) {
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

impl MessageChannel for WhatsAppChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::WhatsApp
    }

    fn status(&self) -> ChannelStatus {
        self.shared.lock().unwrap().status.clone().unwrap_or(ChannelStatus::Disconnected)
    }

    fn can_reach(&self, customer: &Customer) -> bool {
        whatsapp_number(&customer.phone).is_some()
    }

    fn send(&self, customer: &Customer, text: &str) -> Result<(), ChannelError> {
        let number = whatsapp_number(&customer.phone).ok_or(ChannelError::NoContact)?;
        let client = self.shared.lock().unwrap().client.clone().ok_or(ChannelError::NotReady)?;
        self.rt()
            .block_on(client.send_message(Jid::pn(&number), wa::Message::text(text)))
            .map(|_| ())
            .map_err(|e| ChannelError::Failed(e.to_string()))
    }
}
