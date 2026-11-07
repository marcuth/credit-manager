//! Senha do sistema: criação, desbloqueio, troca e cifragem dos segredos guardados em `settings`.
//!
//! O banco guarda só o sal e uma "prova" cifrada (`vault_check`) para saber se a senha digitada
//! está certa. A chave em si nunca é gravada.

use crate::crypto::{self, CryptoError, SALT_LEN, Vault};
use crate::db::{Db, DbError};

/// Configurações que contêm segredos e por isso são cifradas quando há senha.
pub const SECRET_KEYS: &[&str] = &[
    "mp_access_token",
    "telegram_api_hash",
    "gdrive_client_secret",
    "gdrive_refresh_token",
];

const CHECK_PLAINTEXT: &str = "fiado-ok";
pub const MIN_PASSWORD_LEN: usize = 6;

#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("senha incorreta")]
    WrongPassword,
    #[error("a senha precisa ter pelo menos {MIN_PASSWORD_LEN} caracteres")]
    TooShort,
    #[error("a senha ainda não foi criada")]
    NotConfigured,
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

pub fn is_configured(db: &Db) -> bool {
    matches!(db.get_setting("vault_salt"), Ok(Some(_)))
}

/// O usuário escolheu usar o sistema sem senha.
pub fn is_skipped(db: &Db) -> bool {
    matches!(db.get_setting("vault_skipped"), Ok(Some(v)) if v == "1")
}

pub fn skip(db: &Db) -> Result<(), SecurityError> {
    db.set_setting("vault_skipped", "1")?;
    Ok(())
}

/// Cria a senha pela primeira vez e cifra os segredos que já estavam em texto puro.
pub fn create(db: &Db, password: &str) -> Result<Vault, SecurityError> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(SecurityError::TooShort);
    }
    let vault = install(db, password)?;
    for key in SECRET_KEYS {
        if let Some(value) = db.get_setting(key)?
            && !value.is_empty()
            && !crypto::is_sealed_text(&value)
        {
            db.set_setting(key, &vault.seal_text(&value)?)?;
        }
    }
    db.set_setting("vault_skipped", "0")?;
    Ok(vault)
}

pub fn unlock(db: &Db, password: &str) -> Result<Vault, SecurityError> {
    let salt = db.get_setting("vault_salt")?.ok_or(SecurityError::NotConfigured)?;
    let check = db.get_setting("vault_check")?.ok_or(SecurityError::NotConfigured)?;
    let vault = Vault::derive(password, &crypto::b64_decode(&salt)?)?;
    match vault.open_text(&check) {
        Ok(text) if text == CHECK_PLAINTEXT => Ok(vault),
        _ => Err(SecurityError::WrongPassword),
    }
}

/// Troca a senha re-cifrando os segredos. Backups feitos antes continuam exigindo a senha antiga.
pub fn change_password(db: &Db, old: &str, new: &str) -> Result<Vault, SecurityError> {
    if new.chars().count() < MIN_PASSWORD_LEN {
        return Err(SecurityError::TooShort);
    }
    let old_vault = unlock(db, old)?;
    let mut plain = Vec::new();
    for key in SECRET_KEYS {
        if let Some(value) = db.get_setting(key)? {
            plain.push((*key, old_vault.open_text(&value)?));
        }
    }
    let new_vault = install(db, new)?;
    for (key, value) in plain {
        let stored = if value.is_empty() { value } else { new_vault.seal_text(&value)? };
        db.set_setting(key, &stored)?;
    }
    Ok(new_vault)
}

/// Gera sal novo e grava a prova de senha.
fn install(db: &Db, password: &str) -> Result<Vault, SecurityError> {
    let salt = crypto::random_bytes::<SALT_LEN>()?;
    let vault = Vault::derive(password, &salt)?;
    db.set_setting("vault_salt", &crypto::b64_encode(&salt))?;
    db.set_setting("vault_check", &vault.seal_text(CHECK_PLAINTEXT)?)?;
    Ok(vault)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_unlock() {
        let db = Db::open_in_memory().unwrap();
        assert!(!is_configured(&db));
        create(&db, "minha-senha").unwrap();
        assert!(is_configured(&db));
        assert!(unlock(&db, "minha-senha").is_ok());
        assert!(matches!(unlock(&db, "outra-coisa"), Err(SecurityError::WrongPassword)));
    }

    #[test]
    fn rejects_short_password() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(create(&db, "123"), Err(SecurityError::TooShort)));
        assert!(!is_configured(&db));
    }

    #[test]
    fn existing_plaintext_secrets_get_sealed() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting("mp_access_token", "APP_USR-segredo").unwrap();
        let vault = create(&db, "minha-senha").unwrap();
        let stored = db.get_setting("mp_access_token").unwrap().unwrap();
        assert!(crypto::is_sealed_text(&stored));
        assert!(!stored.contains("segredo"));
        assert_eq!(vault.open_text(&stored).unwrap(), "APP_USR-segredo");
    }

    #[test]
    fn change_password_reseals_secrets() {
        let db = Db::open_in_memory().unwrap();
        let v1 = create(&db, "senha-antiga").unwrap();
        db.set_setting("mp_access_token", &v1.seal_text("token").unwrap()).unwrap();

        assert!(matches!(change_password(&db, "errada", "senha-nova"), Err(SecurityError::WrongPassword)));
        let v2 = change_password(&db, "senha-antiga", "senha-nova").unwrap();

        assert!(matches!(unlock(&db, "senha-antiga"), Err(SecurityError::WrongPassword)));
        let stored = db.get_setting("mp_access_token").unwrap().unwrap();
        assert_eq!(v2.open_text(&stored).unwrap(), "token");
        assert_eq!(unlock(&db, "senha-nova").unwrap().open_text(&stored).unwrap(), "token");
    }

    #[test]
    fn skipping_is_remembered() {
        let db = Db::open_in_memory().unwrap();
        assert!(!is_skipped(&db));
        skip(&db).unwrap();
        assert!(is_skipped(&db));
    }
}
