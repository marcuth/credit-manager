use super::{ChannelError, ChannelStatus, MessageChannel, whatsapp_number};
use crate::db::Customer;
use grammers_client::{
    Client, SenderPool, SignInError,
    client::{LoginToken, PasswordToken},
    peer::User,
    sender::SenderPoolFatHandle,
    tl,
};
use grammers_session::{Session, SessionData, storages::MemorySession, types::DcOption};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// O que precisamos guardar para continuar logado: o DC principal e as chaves de autorização.
#[derive(Serialize, Deserialize)]
struct StoredSession {
    home_dc: i32,
    dc_options: Vec<DcOption>,
}

fn load_session(path: &Path) -> Arc<MemorySession> {
    let mut data = SessionData::default();
    if let Ok(text) = std::fs::read_to_string(path)
        && let Ok(stored) = serde_json::from_str::<StoredSession>(&text)
    {
        data.home_dc = stored.home_dc;
        for dc in stored.dc_options {
            data.dc_options.insert(dc.id, dc);
        }
    }
    Arc::new(MemorySession::from(data))
}

fn save_session(session: &MemorySession, path: &Path) {
    let Ok(home_dc) = session.home_dc_id() else { return };
    let dc_options: Vec<DcOption> = (1..=5).filter_map(|id| session.dc_option(id).ok().flatten()).collect();
    if let Ok(json) = serde_json::to_string(&StoredSession { home_dc, dc_options }) {
        let _ = std::fs::write(path, json);
    }
}

struct Conn {
    client: Client,
    handle: SenderPoolFatHandle,
    session: Arc<MemorySession>,
}

struct Shared {
    status: ChannelStatus,
    conn: Option<Conn>,
    login: Option<LoginToken>,
    password: Option<PasswordToken>,
    /// Último erro de login, para a tela mostrar sem perder a etapa em que o usuário está.
    last_error: Option<String>,
}

/// Telegram pela **conta do usuário** (protocolo MTProto, crate `grammers`), não por bot.
/// Assim dá para escrever a qualquer número de telefone que tenha Telegram.
///
/// Exige `api_id` e `api_hash` do próprio usuário (https://my.telegram.org). Roda num runtime
/// tokio próprio; a interface só dispara operações e lê o status.
pub struct TelegramChannel {
    runtime: Option<tokio::runtime::Runtime>,
    session_path: PathBuf,
    shared: Arc<Mutex<Shared>>,
}

impl TelegramChannel {
    pub fn new(session_path: PathBuf) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        Ok(Self {
            runtime: Some(runtime),
            session_path,
            shared: Arc::new(Mutex::new(Shared {
                status: ChannelStatus::NotConfigured,
                conn: None,
                login: None,
                password: None,
                last_error: None,
            })),
        })
    }

    fn rt(&self) -> &tokio::runtime::Runtime {
        self.runtime.as_ref().expect("runtime vivo até o Drop")
    }

    pub fn last_error(&self) -> Option<String> {
        self.shared.lock().unwrap().last_error.clone()
    }

    /// Existe uma sessão salva (conta já conectada neste computador)?
    pub fn has_session(&self) -> bool {
        self.session_path.exists()
    }

    /// Conecta e verifica a sessão salva. Se não houver login, o status vira `NeedsLogin`.
    pub fn connect(&self, api_id: i32) {
        self.start(api_id, None);
    }

    /// Conecta (se preciso) e pede o código de login ao Telegram para o `phone`.
    pub fn start_login(&self, api_id: i32, api_hash: String, phone: String) {
        self.start(api_id, Some((api_hash, phone)));
    }

    fn start(&self, api_id: i32, login: Option<(String, String)>) {
        {
            let mut g = self.shared.lock().unwrap();
            if matches!(
                g.status,
                ChannelStatus::Connecting | ChannelStatus::Ready | ChannelStatus::AwaitingPassword(_)
            ) {
                return;
            }
            // Já conectado e sem pedido de login: nada a fazer.
            if g.conn.is_some() && login.is_none() {
                return;
            }
            g.status = ChannelStatus::Connecting;
            g.last_error = None;
        }
        let (shared, path) = (self.shared.clone(), self.session_path.clone());
        self.rt().spawn(async move {
            let existing = shared.lock().unwrap().conn.as_ref().map(|c| (c.client.clone(), c.session.clone()));
            let (client, session) = match existing {
                Some(pair) => pair,
                None => {
                    let session = load_session(&path);
                    let SenderPool { runner, updates, handle } = SenderPool::new(session.clone(), api_id);
                    let client = Client::new(handle.clone());
                    tokio::spawn(runner.run());
                    // Conta de usuário recebe todas as mensagens: descartamos os updates para não acumular.
                    tokio::spawn(async move {
                        let mut updates = updates;
                        while updates.recv().await.is_some() {}
                    });
                    shared.lock().unwrap().conn = Some(Conn { client: client.clone(), handle, session: session.clone() });
                    (client, session)
                }
            };
            match client.is_authorized().await {
                Ok(true) => {
                    save_session(&session, &path);
                    shared.lock().unwrap().status = ChannelStatus::Ready;
                }
                Ok(false) => match login {
                    Some((api_hash, phone)) => match client.request_login_code(&phone, &api_hash).await {
                        Ok(token) => {
                            let mut g = shared.lock().unwrap();
                            g.login = Some(token);
                            g.status = ChannelStatus::AwaitingCode;
                        }
                        Err(e) => fail(&shared, ChannelStatus::NeedsLogin, format!("Não foi possível pedir o código: {e}")),
                    },
                    None => shared.lock().unwrap().status = ChannelStatus::NeedsLogin,
                },
                Err(e) => fail(&shared, ChannelStatus::Error(e.to_string()), e.to_string()),
            }
        });
    }

    /// Envia o código recebido no Telegram (ou por SMS).
    pub fn submit_code(&self, code: String) {
        let (shared, path) = (self.shared.clone(), self.session_path.clone());
        let (client, session, token) = {
            let mut g = shared.lock().unwrap();
            let Some(token) = g.login.take() else { return };
            let Some(conn) = g.conn.as_ref() else { return };
            let triple = (conn.client.clone(), conn.session.clone(), token);
            g.status = ChannelStatus::Connecting;
            g.last_error = None;
            triple
        };
        self.rt().spawn(async move {
            match client.sign_in(&token, code.trim()).await {
                Ok(_) => {
                    save_session(&session, &path);
                    let mut g = shared.lock().unwrap();
                    g.last_error = None;
                    g.status = ChannelStatus::Ready;
                }
                Err(SignInError::PasswordRequired(pt)) => {
                    let hint = pt.hint().map(str::to_string);
                    let mut g = shared.lock().unwrap();
                    g.password = Some(pt);
                    g.status = ChannelStatus::AwaitingPassword(hint);
                }
                Err(SignInError::InvalidCode) => {
                    shared.lock().unwrap().login = Some(token);
                    fail(&shared, ChannelStatus::AwaitingCode, "Código inválido. Confira e tente de novo.".into());
                }
                Err(SignInError::SignUpRequired) => fail(
                    &shared,
                    ChannelStatus::NeedsLogin,
                    "Este número não tem conta no Telegram. Crie a conta no aplicativo oficial primeiro.".into(),
                ),
                Err(e) => fail(&shared, ChannelStatus::NeedsLogin, format!("Falha no login: {e}")),
            }
        });
    }

    /// Envia a senha da verificação em duas etapas.
    pub fn submit_password(&self, password: String) {
        let (shared, path) = (self.shared.clone(), self.session_path.clone());
        let (client, session, token) = {
            let mut g = shared.lock().unwrap();
            let Some(token) = g.password.take() else { return };
            let Some(conn) = g.conn.as_ref() else { return };
            let triple = (conn.client.clone(), conn.session.clone(), token);
            g.status = ChannelStatus::Connecting;
            g.last_error = None;
            triple
        };
        self.rt().spawn(async move {
            match client.check_password(token, password).await {
                Ok(_) => {
                    save_session(&session, &path);
                    let mut g = shared.lock().unwrap();
                    g.last_error = None;
                    g.status = ChannelStatus::Ready;
                }
                Err(SignInError::InvalidPassword(pt)) => {
                    let hint = pt.hint().map(str::to_string);
                    shared.lock().unwrap().password = Some(pt);
                    fail(&shared, ChannelStatus::AwaitingPassword(hint), "Senha incorreta.".into());
                }
                Err(e) => fail(&shared, ChannelStatus::NeedsLogin, format!("Falha no login: {e}")),
            }
        });
    }

    /// Desliga a conexão mantendo a sessão salva.
    pub fn disconnect(&self) {
        let mut g = self.shared.lock().unwrap();
        if let Some(conn) = g.conn.take() {
            save_session(&conn.session, &self.session_path);
            conn.handle.quit();
        }
        g.login = None;
        g.password = None;
        g.status = ChannelStatus::Disconnected;
    }

    /// Sai da conta no Telegram e apaga a sessão salva.
    pub fn sign_out(&self) {
        let conn = {
            let mut g = self.shared.lock().unwrap();
            g.login = None;
            g.password = None;
            g.status = ChannelStatus::Disconnected;
            g.conn.take()
        };
        let path = self.session_path.clone();
        self.rt().spawn(async move {
            if let Some(conn) = conn {
                let _ = conn.client.sign_out().await;
                conn.handle.quit();
            }
            let _ = std::fs::remove_file(path);
        });
    }
}

fn fail(shared: &Mutex<Shared>, status: ChannelStatus, message: String) {
    let mut g = shared.lock().unwrap();
    g.last_error = Some(message);
    g.status = status;
}

impl Drop for TelegramChannel {
    fn drop(&mut self) {
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

impl MessageChannel for TelegramChannel {
    fn status(&self) -> ChannelStatus {
        self.shared.lock().unwrap().status.clone()
    }

    fn can_reach(&self, customer: &Customer) -> bool {
        whatsapp_number(&customer.phone).is_some()
    }

    fn send(&self, customer: &Customer, text: &str) -> Result<(), ChannelError> {
        let number = whatsapp_number(&customer.phone).ok_or(ChannelError::NoContact)?;
        let (client, session) = {
            let g = self.shared.lock().unwrap();
            if g.status != ChannelStatus::Ready {
                return Err(ChannelError::NotReady);
            }
            let conn = g.conn.as_ref().ok_or(ChannelError::NotReady)?;
            (conn.client.clone(), conn.session.clone())
        };
        let (name, customer_id) = (customer.name.clone(), customer.id);
        let result: Result<(), ChannelError> = self.rt().block_on(async move {
            // Importar o número como contato é o jeito de o Telegram resolver telefone -> usuário.
            let imported = client
                .invoke(&tl::functions::contacts::ImportContacts {
                    contacts: vec![tl::enums::InputContact::InputPhoneContact(tl::types::InputPhoneContact {
                        client_id: customer_id,
                        phone: format!("+{number}"),
                        first_name: name,
                        last_name: String::new(),
                        note: None,
                    })],
                })
                .await
                .map_err(|e| ChannelError::Failed(e.to_string()))?;
            let tl::enums::contacts::ImportedContacts::Contacts(imported) = imported;
            let user = imported
                .users
                .into_iter()
                .next()
                .ok_or_else(|| ChannelError::Failed("este número não está no Telegram".into()))?;
            let peer = User::from_raw(&client, user)
                .to_ref()
                .await
                .map_err(|e| ChannelError::Failed(e.to_string()))?
                .ok_or_else(|| ChannelError::Failed("não foi possível localizar o usuário".into()))?;
            client.send_message(peer, text).await.map_err(|e| ChannelError::Failed(e.to_string()))?;
            Ok(())
        });
        save_session(&session, &self.session_path);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_survives_save_and_load() {
        let path = std::env::temp_dir().join("credit-manager-test-tg-session.json");
        let _ = std::fs::remove_file(&path);
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();

        let session = MemorySession::default();
        let mut dc = session.dc_option(4).unwrap().expect("DC 4 é conhecido");
        dc.auth_key = Some([7u8; 256]);
        rt.block_on(async {
            session.set_dc_option(&dc).await.unwrap();
            session.set_home_dc_id(4).await.unwrap();
        });
        save_session(&session, &path);

        let loaded = load_session(&path);
        assert_eq!(loaded.home_dc_id().unwrap(), 4);
        assert_eq!(loaded.dc_option(4).unwrap().unwrap().auth_key, Some([7u8; 256]));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_session_file_starts_fresh() {
        let loaded = load_session(Path::new("nao-existe.json"));
        assert!(loaded.dc_option(2).unwrap().unwrap().auth_key.is_none());
    }
}
