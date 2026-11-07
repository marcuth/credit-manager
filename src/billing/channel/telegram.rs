use super::{ChannelError, ChannelStatus, MessageChannel};
use crate::db::Customer;
use crate::services::Event;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::time::Duration;

struct State {
    token: Option<String>,
    username: Option<String>,
    status: ChannelStatus,
    /// Para a thread de consulta anterior quando a configuração muda.
    stop: Arc<AtomicBool>,
}

/// Telegram via Bot API. Bots não iniciam conversa pelo telefone: o cliente abre o link
/// `t.me/<bot>?start=<código>` e o poller recebe o `/start <código>` para vincular o chat.
pub struct TelegramChannel {
    state: Arc<Mutex<State>>,
}

fn api(token: &str, method: &str) -> String {
    format!("https://api.telegram.org/bot{token}/{method}")
}

fn err(e: ureq::Error) -> String {
    // A mensagem do ureq não inclui a URL (e, portanto, o token).
    e.to_string()
}

impl TelegramChannel {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                token: None,
                username: None,
                status: ChannelStatus::NotConfigured,
                stop: Arc::new(AtomicBool::new(true)),
            })),
        }
    }

    /// Username do bot (sem @), quando já validado.
    pub fn username(&self) -> Option<String> {
        self.state.lock().unwrap().username.clone()
    }

    /// (Re)configura o token: valida com `getMe` e passa a escutar os `/start`.
    pub fn configure(&self, token: &str, events: Sender<Event>) {
        let token = token.trim().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut st = self.state.lock().unwrap();
            st.stop.store(true, Ordering::SeqCst);
            st.stop = stop.clone();
            st.username = None;
            if token.is_empty() {
                st.token = None;
                st.status = ChannelStatus::NotConfigured;
                return;
            }
            st.token = Some(token.clone());
            st.status = ChannelStatus::Connecting;
        }
        let state = self.state.clone();
        std::thread::spawn(move || poll_loop(state, token, stop, events));
    }

    /// Envia direto para um chat (usado na confirmação do vínculo).
    pub fn send_to_chat(&self, chat_id: i64, text: &str) -> Result<(), ChannelError> {
        let token = {
            let st = self.state.lock().unwrap();
            if st.status != ChannelStatus::Ready {
                return Err(ChannelError::NotReady);
            }
            st.token.clone().ok_or(ChannelError::NotReady)?
        };
        ureq::post(api(&token, "sendMessage"))
            .send_json(json!({ "chat_id": chat_id, "text": text }))
            .map_err(|e| ChannelError::Failed(err(e)))?;
        Ok(())
    }
}

fn set_if_current(state: &Mutex<State>, stop: &AtomicBool, f: impl FnOnce(&mut State)) {
    if !stop.load(Ordering::SeqCst) {
        f(&mut state.lock().unwrap());
    }
}

fn poll_loop(state: Arc<Mutex<State>>, token: String, stop: Arc<AtomicBool>, events: Sender<Event>) {
    let me: Result<Value, String> = ureq::get(api(&token, "getMe"))
        .call()
        .map_err(err)
        .and_then(|mut r| r.body_mut().read_json().map_err(err));
    match me {
        Ok(v) if v["ok"] == true => {
            let name = v["result"]["username"].as_str().map(str::to_string);
            set_if_current(&state, &stop, |st| {
                st.username = name;
                st.status = ChannelStatus::Ready;
            });
        }
        Ok(_) => {
            set_if_current(&state, &stop, |st| st.status = ChannelStatus::Error("token recusado pelo Telegram".into()));
            return;
        }
        Err(e) => {
            set_if_current(&state, &stop, |st| st.status = ChannelStatus::Error(e));
            return;
        }
    }

    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(40))).build().into();
    let mut offset: i64 = 0;
    while !stop.load(Ordering::SeqCst) {
        let resp = agent
            .get(api(&token, "getUpdates"))
            .query("offset", offset.to_string())
            .query("timeout", "25")
            .query("allowed_updates", r#"["message"]"#)
            .call()
            .map_err(err)
            .and_then(|mut r| r.body_mut().read_json::<Value>().map_err(err));
        match resp {
            Ok(v) => {
                for u in v["result"].as_array().into_iter().flatten() {
                    offset = offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
                    let text = u["message"]["text"].as_str().unwrap_or("");
                    let chat = u["message"]["chat"]["id"].as_i64();
                    if let (Some(code), Some(chat_id)) = (text.strip_prefix("/start "), chat)
                        && !stop.load(Ordering::SeqCst)
                    {
                        let _ = events.send(Event::TelegramLinked { code: code.trim().to_string(), chat_id });
                    }
                }
            }
            Err(_) => std::thread::sleep(Duration::from_secs(5)),
        }
    }
}

impl MessageChannel for TelegramChannel {
    fn status(&self) -> ChannelStatus {
        self.state.lock().unwrap().status.clone()
    }

    fn can_reach(&self, customer: &Customer) -> bool {
        customer.telegram_chat_id.is_some()
    }

    fn send(&self, customer: &Customer, text: &str) -> Result<(), ChannelError> {
        let chat_id: i64 = customer
            .telegram_chat_id
            .as_deref()
            .and_then(|c| c.parse().ok())
            .ok_or(ChannelError::NoContact)?;
        self.send_to_chat(chat_id, text)
    }
}
