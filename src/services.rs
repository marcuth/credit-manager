use crate::backup::{
    self,
    gdrive::{self, DriveAuth, DriveError, RemoteBackup},
};
use crate::billing::channel::{ChannelError, ChannelKind, Channels};
use crate::billing::{
    BillingError, Checkout, PaymentProvider, PaymentStatus,
    factory::{ProviderKind, create_provider},
};
use crate::crypto::{self, Vault};
use crate::db::{Customer, Db, DbError};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc::Sender},
};

/// Intervalo do backup automático.
const BACKUP_EVERY_SECS: i64 = 24 * 60 * 60;

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
    /// Pedido para abrir uma página no navegador (a thread de login não tem acesso à janela).
    OpenUrl(String),
    DriveLogin(Result<DriveAuth, DriveError>),
    DriveList(Result<Vec<RemoteBackup>, DriveError>),
    DriveUploaded(Result<(), DriveError>),
    DriveDownloaded(Result<Vec<u8>, DriveError>),
}

/// Backup baixado, esperando a confirmação do usuário para restaurar.
pub struct PendingRestore {
    pub source: String,
    pub data: Vec<u8>,
}

/// Tudo o que as telas compartilham.
pub struct Services {
    pub db: Db,
    pub data_dir: PathBuf,
    /// Chave da senha do sistema; `None` quando o usuário optou por não usar senha.
    pub vault: Option<Vault>,
    pub provider: Result<Arc<dyn PaymentProvider>, BillingError>,
    pub events: Sender<Event>,
    pub channels: Channels,
    pub drive: Option<DriveAuth>,
    pub drive_files: Vec<RemoteBackup>,
    pub drive_busy: bool,
    pub pending_restore: Option<PendingRestore>,
    /// Aumenta a cada mudança nos dados; as telas recarregam quando muda.
    pub revision: u64,
    pub status: Option<String>,
}

impl Services {
    pub fn new(db: Db, events: Sender<Event>, data_dir: &Path, vault: Option<Vault>) -> std::io::Result<Self> {
        let mut s = Self {
            db,
            data_dir: data_dir.to_path_buf(),
            vault,
            provider: Err(BillingError::Config("não carregado".into())),
            events,
            channels: Channels::new(data_dir)?,
            drive: None,
            drive_files: Vec::new(),
            drive_busy: false,
            pending_restore: None,
            revision: 0,
            status: None,
        };
        s.reload_provider();
        s.load_drive();
        // Reativa os canais que já estavam configurados.
        let telegram_id = s.db.get_setting("telegram_api_id").ok().flatten().and_then(|v| v.parse::<i32>().ok());
        if let Some(api_id) = telegram_id
            && s.db.get_setting("telegram_enabled").ok().flatten().as_deref() == Some("1")
        {
            s.channels.telegram.connect(api_id);
        }
        if s.db.get_setting("whatsapp_enabled").ok().flatten().as_deref() == Some("1") {
            s.channels.whatsapp.connect();
        }
        Ok(s)
    }

    // ------------------------------------------------------------ segredos

    /// Lê uma configuração secreta (decifrando, se houver senha). Vazio se não existir
    /// ou se não der para decifrar.
    pub fn secret(&self, key: &str) -> String {
        let raw = self.db.get_setting(key).ok().flatten().unwrap_or_default();
        match &self.vault {
            Some(vault) => vault.open_text(&raw).unwrap_or_default(),
            None if crypto::is_sealed_text(&raw) => String::new(),
            None => raw,
        }
    }

    pub fn set_secret(&self, key: &str, value: &str) -> Result<(), DbError> {
        let stored = match (&self.vault, value.is_empty()) {
            (Some(vault), false) => vault.seal_text(value).map_err(|e| DbError::Invalid(e.to_string()))?,
            _ => value.to_string(),
        };
        self.db.set_setting(key, &stored)
    }

    // ------------------------------------------------------------ canais

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
        let token = self.secret("mp_access_token");
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

    // ------------------------------------------------------------ backup

    pub fn auto_backup_enabled(&self) -> bool {
        // Ligado por padrão.
        self.db.get_setting("backup_auto").ok().flatten().as_deref() != Some("0")
    }

    pub fn drive_auto_enabled(&self) -> bool {
        self.db.get_setting("gdrive_auto").ok().flatten().as_deref() != Some("0")
    }

    /// Instante (segundos Unix) do último backup local, se houver.
    pub fn last_backup_at(&self) -> Option<i64> {
        self.db.get_setting("last_backup_at").ok().flatten().and_then(|v| v.parse().ok())
    }

    pub fn backup_due(&self) -> bool {
        if !self.auto_backup_enabled() {
            return false;
        }
        match self.last_backup_at() {
            Some(last) => chrono::Utc::now().timestamp() - last >= BACKUP_EVERY_SECS,
            None => true,
        }
    }

    /// Cria um backup local agora e, se o Drive estiver conectado e o envio automático ligado,
    /// envia uma cópia cifrada.
    pub fn run_backup(&mut self) {
        match backup::create_local(&self.db, &self.data_dir, backup::LOCAL_KEEP) {
            Ok(_) => {
                let _ = self.db.set_setting("last_backup_at", &chrono::Utc::now().timestamp().to_string());
                self.notify("Backup criado.");
                self.changed();
            }
            Err(e) => {
                self.notify(format!("Falha ao criar o backup: {e}"));
                return;
            }
        }
        if self.drive.is_some() && self.vault.is_some() && self.drive_auto_enabled() {
            self.drive_upload_now();
        }
    }

    /// Restaura os dados a partir do conteúdo de um backup (cifrado ou não).
    pub fn restore_backup(&mut self, data: Vec<u8>, password: Option<&str>) -> Result<(), backup::BackupError> {
        let salt = backup::current_salt(&self.db);
        let plain = backup::open_with_session(data, self.vault.as_ref(), salt.as_deref(), password)?;
        backup::restore_plain(&mut self.db, &self.data_dir, plain)?;
        self.changed();
        Ok(())
    }

    // ------------------------------------------------------------ Google Drive

    pub fn load_drive(&mut self) {
        let client_id = self.db.get_setting("gdrive_client_id").ok().flatten().unwrap_or_default();
        let client_secret = self.secret("gdrive_client_secret");
        let refresh_token = self.secret("gdrive_refresh_token");
        self.drive = (!client_id.is_empty() && !client_secret.is_empty() && !refresh_token.is_empty())
            .then_some(DriveAuth { client_id, client_secret, refresh_token });
    }

    pub fn drive_login(&mut self, client_id: String, client_secret: String) {
        self.drive_busy = true;
        let tx = self.events.clone();
        std::thread::spawn(move || {
            let tx_url = tx.clone();
            let result = gdrive::login(&client_id, &client_secret, move |url| {
                let _ = tx_url.send(Event::OpenUrl(url.to_string()));
            });
            let _ = tx.send(Event::DriveLogin(result));
        });
    }

    pub fn drive_refresh_list(&mut self) {
        let Some(auth) = self.drive.clone() else { return };
        self.drive_busy = true;
        let tx = self.events.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::DriveList(gdrive::list(&auth)));
        });
    }

    pub fn drive_upload_now(&mut self) {
        let Some(auth) = self.drive.clone() else { return };
        let data = match backup::build_encrypted_snapshot(&self.db, self.vault.as_ref()) {
            Ok(d) => d,
            Err(e) => {
                self.notify(format!("Não foi possível preparar o backup para a nuvem: {e}"));
                return;
            }
        };
        let name = format!("{}.db.enc", backup::timestamp_stem());
        self.drive_busy = true;
        let tx = self.events.clone();
        std::thread::spawn(move || {
            let result = gdrive::upload(&auth, &name, &data).and_then(|_| gdrive::prune(&auth, backup::REMOTE_KEEP));
            let _ = tx.send(Event::DriveUploaded(result));
        });
    }

    pub fn drive_download(&mut self, file_id: String) {
        let Some(auth) = self.drive.clone() else { return };
        self.drive_busy = true;
        let tx = self.events.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::DriveDownloaded(gdrive::download(&auth, &file_id)));
        });
    }

    /// Esquece a conexão com o Drive (os backups já enviados continuam lá).
    pub fn drive_disconnect(&mut self) {
        let _ = self.db.set_setting("gdrive_refresh_token", "");
        self.drive = None;
        self.drive_files.clear();
    }
}
