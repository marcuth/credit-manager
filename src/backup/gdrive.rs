//! Google Drive: login OAuth2 de aplicativo desktop (PKCE + redirecionamento para `127.0.0.1`)
//! e envio, listagem, download e limpeza de backups.
//!
//! Usa o escopo `drive.file`: o aplicativo só enxerga os arquivos que ele mesmo criou, e nunca o
//! restante do Drive. O usuário precisa criar o próprio cliente OAuth do tipo "Aplicativo para
//! computador" no Google Cloud (ver README).

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64URL};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const FOLDER_NAME: &str = "Gestor de Credito - Backups";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, thiserror::Error)]
pub enum DriveError {
    #[error("falha na comunicação com o Google: {0}")]
    Http(String),
    #[error("resposta inesperada do Google: {0}")]
    Parse(String),
    #[error("login cancelado ou expirado")]
    LoginFailed,
    #[error("a conexão com o Google Drive expirou: conecte de novo")]
    Reauthorize,
}

fn http(e: ureq::Error) -> DriveError {
    match e {
        ureq::Error::StatusCode(401) => DriveError::Reauthorize,
        other => DriveError::Http(other.to_string()),
    }
}

#[derive(Debug, Clone)]
pub struct RemoteBackup {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub created: String,
}

/// Credenciais do cliente OAuth criado pelo usuário e o token de renovação (depois do login).
#[derive(Debug, Clone)]
pub struct DriveAuth {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() + 0 && i + 2 <= bytes.len() - 1 => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v))).collect::<Vec<_>>().join("&")
}

fn post_form(url: &str, pairs: &[(&str, &str)]) -> Result<Value, DriveError> {
    let mut resp = ureq::post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(form(pairs).as_bytes())
        .map_err(http)?;
    resp.body_mut().read_json().map_err(http)
}

/// Dados do PKCE: `(verifier, challenge)`.
fn pkce() -> Result<(String, String), crate::crypto::CryptoError> {
    let verifier = B64URL.encode(crate::crypto::random_bytes::<32>()?);
    let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
    Ok((verifier, challenge))
}

/// Parâmetros de uma requisição HTTP simples (`GET /?code=...&state=...`).
fn parse_callback(request_line: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let path = request_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;
    let (mut code, mut state, mut error) = (None, None, None);
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "code" => code = Some(url_decode(v)),
            "state" => state = Some(url_decode(v)),
            "error" => error = Some(url_decode(v)),
            _ => {}
        }
    }
    Some((code, state, error))
}

/// Faz o login completo: abre o navegador (via `open_url`), espera o retorno do Google e troca
/// o código pelo token de renovação. Bloqueante (até 3 minutos): rode numa thread.
pub fn login(client_id: &str, client_secret: &str, open_url: impl FnOnce(&str)) -> Result<DriveAuth, DriveError> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| DriveError::Http(e.to_string()))?;
    listener.set_nonblocking(true).map_err(|e| DriveError::Http(e.to_string()))?;
    let port = listener.local_addr().map_err(|e| DriveError::Http(e.to_string()))?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let (verifier, challenge) = pkce().map_err(|e| DriveError::Http(e.to_string()))?;
    let state = B64URL.encode(crate::crypto::random_bytes::<16>().map_err(|e| DriveError::Http(e.to_string()))?);

    let url = format!(
        "{AUTH_URL}?{}",
        form(&[
            ("client_id", client_id),
            ("redirect_uri", &redirect),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ])
    );
    open_url(&url);

    let started = Instant::now();
    let code = loop {
        if started.elapsed() > LOGIN_TIMEOUT {
            return Err(DriveError::LoginFailed);
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let parsed = request.lines().next().and_then(parse_callback);
                let ok = matches!(&parsed, Some((Some(_), Some(s), _)) if *s == state);
                let body = if ok {
                    "<html><body style='font-family:sans-serif'><h2>Pronto!</h2><p>Conexão com o Google Drive concluída. Pode fechar esta aba e voltar ao sistema.</p></body></html>"
                } else {
                    "<html><body style='font-family:sans-serif'><h2>Não foi possível conectar.</h2><p>Volte ao sistema e tente de novo.</p></body></html>"
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                match parsed {
                    Some((Some(code), Some(s), _)) if s == state => break code,
                    Some((_, _, Some(_))) => return Err(DriveError::LoginFailed),
                    _ => continue, // requisição estranha (ex.: favicon): continua esperando
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(150)),
            Err(e) => return Err(DriveError::Http(e.to_string())),
        }
    };

    let tokens = post_form(
        TOKEN_URL,
        &[
            ("code", &code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", &redirect),
            ("grant_type", "authorization_code"),
            ("code_verifier", &verifier),
        ],
    )?;
    let refresh_token = tokens["refresh_token"]
        .as_str()
        .ok_or_else(|| DriveError::Parse("o Google não devolveu o token de renovação".into()))?;
    Ok(DriveAuth {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        refresh_token: refresh_token.to_string(),
    })
}

pub fn access_token(auth: &DriveAuth) -> Result<String, DriveError> {
    let v = post_form(
        TOKEN_URL,
        &[
            ("client_id", &auth.client_id),
            ("client_secret", &auth.client_secret),
            ("refresh_token", &auth.refresh_token),
            ("grant_type", "refresh_token"),
        ],
    )
    .map_err(|e| match e {
        // 400 (invalid_grant) = token revogado ou expirado
        DriveError::Http(msg) if msg.contains("400") => DriveError::Reauthorize,
        other => other,
    })?;
    v["access_token"].as_str().map(str::to_string).ok_or_else(|| DriveError::Parse("sem access_token".into()))
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// Id da pasta de backups (cria se não existir).
fn folder_id(token: &str) -> Result<String, DriveError> {
    let q = format!("name = '{FOLDER_NAME}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false");
    let mut resp = ureq::get(FILES_URL)
        .header("Authorization", bearer(token))
        .query("q", &q)
        .query("fields", "files(id)")
        .call()
        .map_err(http)?;
    let v: Value = resp.body_mut().read_json().map_err(http)?;
    if let Some(id) = v["files"][0]["id"].as_str() {
        return Ok(id.to_string());
    }
    let mut resp = ureq::post(FILES_URL)
        .header("Authorization", bearer(token))
        .send_json(json!({ "name": FOLDER_NAME, "mimeType": "application/vnd.google-apps.folder" }))
        .map_err(http)?;
    let v: Value = resp.body_mut().read_json().map_err(http)?;
    v["id"].as_str().map(str::to_string).ok_or_else(|| DriveError::Parse("pasta sem id".into()))
}

pub fn upload(auth: &DriveAuth, name: &str, data: &[u8]) -> Result<(), DriveError> {
    let token = access_token(auth)?;
    let folder = folder_id(&token)?;
    let boundary = B64URL.encode(crate::crypto::random_bytes::<12>().map_err(|e| DriveError::Http(e.to_string()))?);
    let metadata = json!({ "name": name, "parents": [folder] }).to_string();
    let mut body = Vec::new();
    body.extend(format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n").into_bytes());
    body.extend(format!("--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes());
    body.extend_from_slice(data);
    body.extend(format!("\r\n--{boundary}--").into_bytes());
    ureq::post(format!("{UPLOAD_URL}?uploadType=multipart"))
        .header("Authorization", bearer(&token))
        .header("Content-Type", format!("multipart/related; boundary={boundary}"))
        .send(&body[..])
        .map_err(http)?;
    Ok(())
}

pub fn list(auth: &DriveAuth) -> Result<Vec<RemoteBackup>, DriveError> {
    let token = access_token(auth)?;
    let folder = folder_id(&token)?;
    let q = format!("'{folder}' in parents and trashed = false");
    let mut resp = ureq::get(FILES_URL)
        .header("Authorization", bearer(&token))
        .query("q", &q)
        .query("orderBy", "createdTime desc")
        .query("pageSize", "100")
        .query("fields", "files(id,name,size,createdTime)")
        .call()
        .map_err(http)?;
    let v: Value = resp.body_mut().read_json().map_err(http)?;
    Ok(v["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            Some(RemoteBackup {
                id: f["id"].as_str()?.to_string(),
                name: f["name"].as_str()?.to_string(),
                size: f["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                created: f["createdTime"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect())
}

pub fn download(auth: &DriveAuth, file_id: &str) -> Result<Vec<u8>, DriveError> {
    let token = access_token(auth)?;
    let mut resp = ureq::get(format!("{FILES_URL}/{file_id}"))
        .header("Authorization", bearer(&token))
        .query("alt", "media")
        .call()
        .map_err(http)?;
    resp.body_mut().with_config().limit(512 * 1024 * 1024).read_to_vec().map_err(http)
}

pub fn delete(auth: &DriveAuth, file_id: &str) -> Result<(), DriveError> {
    let token = access_token(auth)?;
    ureq::delete(format!("{FILES_URL}/{file_id}")).header("Authorization", bearer(&token)).call().map_err(http)?;
    Ok(())
}

/// Apaga os backups mais antigos além de `keep` (lista já ordenada do mais novo ao mais antigo).
pub fn prune(auth: &DriveAuth, keep: usize) -> Result<(), DriveError> {
    for old in list(auth)?.into_iter().skip(keep) {
        delete(auth, &old.id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encoding_roundtrip() {
        let original = "http://127.0.0.1:5000 & ção/=?";
        assert_eq!(url_decode(&url_encode(original)), original);
        assert_eq!(url_encode("a b&c"), "a%20b%26c");
    }

    #[test]
    fn callback_parsing() {
        let (code, state, error) =
            parse_callback("GET /?state=abc&code=4%2F0AX-y HTTP/1.1").unwrap();
        assert_eq!(code.as_deref(), Some("4/0AX-y"));
        assert_eq!(state.as_deref(), Some("abc"));
        assert_eq!(error, None);

        let (code, _, error) = parse_callback("GET /?error=access_denied&state=abc HTTP/1.1").unwrap();
        assert_eq!(code, None);
        assert_eq!(error.as_deref(), Some("access_denied"));

        assert!(parse_callback("GET /favicon.ico HTTP/1.1").is_none());
    }

    #[test]
    fn pkce_challenge_matches_verifier() {
        let (verifier, challenge) = pkce().unwrap();
        assert_eq!(challenge, B64URL.encode(Sha256::digest(verifier.as_bytes())));
        assert!(verifier.len() >= 43, "o verifier precisa ter de 43 a 128 caracteres");
    }

    #[test]
    fn login_flow_rejects_a_wrong_state() {
        // O "navegador" roda noutra thread: `open_url` é chamado antes de o servidor aceitar conexões.
        let result = login("cid", "secret", |url| {
            let port = url
                .split("redirect_uri=http%3A%2F%2F127.0.0.1%3A")
                .nth(1)
                .and_then(|rest| rest.split('&').next())
                .and_then(|p| p.parse::<u16>().ok())
                .expect("porta no redirect_uri");
            std::thread::spawn(move || {
                for path in ["/?code=x&state=errado", "/?error=access_denied&state=errado"] {
                    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                    let mut sink = String::new();
                    let _ = s.read_to_string(&mut sink);
                }
            });
        });
        assert!(matches!(result, Err(DriveError::LoginFailed)));
    }
}
