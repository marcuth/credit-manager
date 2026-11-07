//! Backups do banco: arquivos locais (rotacionados) e o formato cifrado enviado à nuvem.
//!
//! Os backups não levam as configurações (tokens e senha). O formato cifrado é
//! `FIADOBK1 | sal (16 bytes) | nonce+texto cifrado`: o sal vem junto para que uma instalação
//! nova consiga abrir o backup só com a senha.

pub mod gdrive;

use crate::crypto::{self, CryptoError, SALT_LEN, Vault};
use crate::db::{Db, DbError};
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

const MAGIC: &[u8; 8] = b"FIADOBK1";
pub const LOCAL_KEEP: usize = 14;
pub const REMOTE_KEEP: usize = 30;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error("erro de arquivo: {0}")]
    Io(#[from] std::io::Error),
    #[error("este backup é cifrado: informe a senha")]
    PasswordRequired,
    #[error("é preciso criar a senha do sistema para enviar backups à nuvem")]
    NoVault,
}

#[derive(Clone, Debug)]
pub struct LocalBackup {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified: SystemTime,
}

/// Arquivo temporário com nome único (dois backups simultâneos não podem usar o mesmo arquivo).
fn temp_file(prefix: &str) -> PathBuf {
    let unique = crypto::random_bytes::<8>()
        .map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<String>())
        .unwrap_or_else(|_| format!("{:?}", std::time::Instant::now()).replace(['{', '}', ' ', ':', ','], ""));
    std::env::temp_dir().join(format!("{prefix}-{}-{unique}.db", std::process::id()))
}

pub fn backups_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

/// `fiado-20261107-153000` (hora local), sem extensão.
pub fn timestamp_stem() -> String {
    format!("fiado-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"))
}

/// Cria um backup local (`.db`, sem as configurações) e apaga os mais antigos.
pub fn create_local(db: &Db, data_dir: &Path, keep: usize) -> Result<PathBuf, BackupError> {
    let dir = backups_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.db", timestamp_stem()));
    db.snapshot_to(&path, false)?;
    prune_local(data_dir, keep);
    Ok(path)
}

pub fn list_local(data_dir: &Path) -> Vec<LocalBackup> {
    let Ok(entries) = std::fs::read_dir(backups_dir(data_dir)) else { return vec![] };
    let mut backups: Vec<LocalBackup> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "db"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some(LocalBackup {
                name: e.file_name().to_string_lossy().into_owned(),
                path: e.path(),
                size: meta.len(),
                modified: meta.modified().ok()?,
            })
        })
        .collect();
    // O nome tem a data, então ordenar por nome (decrescente) é ordenar do mais novo ao mais antigo.
    backups.sort_by(|a, b| b.name.cmp(&a.name));
    backups
}

pub fn prune_local(data_dir: &Path, keep: usize) {
    for old in list_local(data_dir).into_iter().skip(keep) {
        let _ = std::fs::remove_file(old.path);
    }
}

pub fn is_encrypted(data: &[u8]) -> bool {
    data.starts_with(MAGIC)
}

/// Cifra o conteúdo de um backup com a chave da sessão; `salt` é o sal dessa chave.
pub fn encrypt(vault: &Vault, salt: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, BackupError> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(salt);
    out.extend(vault.seal(plaintext)?);
    Ok(out)
}

/// Abre um backup cifrado usando o sal embutido e a senha informada.
pub fn decrypt(password: &str, data: &[u8]) -> Result<Vec<u8>, BackupError> {
    if !is_encrypted(data) || data.len() < MAGIC.len() + SALT_LEN {
        return Err(CryptoError::Malformed.into());
    }
    let (salt, sealed) = data[MAGIC.len()..].split_at(SALT_LEN);
    Ok(Vault::derive(password, salt)?.open(sealed)?)
}

/// Como [`open_backup_bytes`], mas tenta primeiro a chave da sessão: se o backup foi cifrado
/// com a senha atual (mesmo sal), não precisa pedir a senha de novo.
pub fn open_with_session(
    data: Vec<u8>,
    vault: Option<&Vault>,
    session_salt: Option<&[u8]>,
    password: Option<&str>,
) -> Result<Vec<u8>, BackupError> {
    if is_encrypted(&data)
        && data.len() >= MAGIC.len() + SALT_LEN
        && let (Some(vault), Some(salt)) = (vault, session_salt)
    {
        let (header_salt, sealed) = data[MAGIC.len()..].split_at(SALT_LEN);
        if header_salt == salt {
            return Ok(vault.open(sealed)?);
        }
    }
    open_backup_bytes(data, password)
}

/// O backup é cifrado e a chave da sessão não serve (outro sal)? Então é preciso pedir a senha.
pub fn needs_password(data: &[u8], session_salt: Option<&[u8]>, has_vault: bool) -> bool {
    if !is_encrypted(data) {
        return false;
    }
    if data.len() < MAGIC.len() + SALT_LEN {
        return true;
    }
    let header_salt = &data[MAGIC.len()..MAGIC.len() + SALT_LEN];
    !(has_vault && session_salt == Some(header_salt))
}

/// Lê o conteúdo de um backup (cifrado ou não) e devolve o banco em texto claro.
pub fn open_backup_bytes(data: Vec<u8>, password: Option<&str>) -> Result<Vec<u8>, BackupError> {
    if is_encrypted(&data) {
        decrypt(password.ok_or(BackupError::PasswordRequired)?, &data)
    } else {
        Ok(data)
    }
}

/// Restaura os dados a partir do conteúdo de um backup. Antes, guarda um backup local do estado
/// atual (`antes-de-restaurar`) para dar para desfazer.
pub fn restore(db: &mut Db, data_dir: &Path, data: Vec<u8>, password: Option<&str>) -> Result<(), BackupError> {
    restore_plain(db, data_dir, open_backup_bytes(data, password)?)
}

/// Restaura a partir do conteúdo do banco já em texto claro.
pub fn restore_plain(db: &mut Db, data_dir: &Path, plain: Vec<u8>) -> Result<(), BackupError> {
    let tmp = temp_file("fiado-restore");
    std::fs::write(&tmp, plain)?;
    let result = (|| -> Result<(), BackupError> {
        Db::validate_backup(&tmp)?;
        let dir = backups_dir(data_dir);
        std::fs::create_dir_all(&dir)?;
        db.snapshot_to(&dir.join(format!("{}-antes-de-restaurar.db", timestamp_stem())), false)?;
        db.restore_from(&tmp)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Sal da senha atual (o mesmo usado para derivar a chave da sessão).
pub fn current_salt(db: &Db) -> Option<Vec<u8>> {
    let salt = db.get_setting("vault_salt").ok().flatten()?;
    crypto::b64_decode(&salt).ok()
}

/// Gera o conteúdo cifrado de um backup novo, pronto para enviar à nuvem.
pub fn build_encrypted_snapshot(db: &Db, vault: Option<&Vault>) -> Result<Vec<u8>, BackupError> {
    let vault = vault.ok_or(BackupError::NoVault)?;
    let salt = current_salt(db).ok_or(BackupError::NoVault)?;
    let tmp = temp_file("fiado-upload");
    db.snapshot_to(&tmp, false)?;
    let plain = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    encrypt(vault, &salt, &plain?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Kind;
    use crate::security;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("credit-manager-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn local_backups_are_listed_newest_first_and_pruned() {
        let dir = temp_dir("bk-prune");
        let backups = backups_dir(&dir);
        std::fs::create_dir_all(&backups).unwrap();
        for stamp in ["20260101-100000", "20260301-100000", "20260201-100000", "20260401-100000"] {
            std::fs::write(backups.join(format!("fiado-{stamp}.db")), b"x").unwrap();
        }
        std::fs::write(backups.join("anotacao.txt"), b"ignorar").unwrap();

        let names: Vec<_> = list_local(&dir).into_iter().map(|b| b.name).collect();
        assert_eq!(names[0], "fiado-20260401-100000.db");
        assert_eq!(names.len(), 4);

        prune_local(&dir, 2);
        let names: Vec<_> = list_local(&dir).into_iter().map(|b| b.name).collect();
        assert_eq!(names, ["fiado-20260401-100000.db", "fiado-20260301-100000.db"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn encrypted_backup_roundtrip_needs_the_right_password() {
        let db = Db::open_in_memory().unwrap();
        let vault = security::create(&db, "senha-do-backup").unwrap();
        let id = db.add_customer("Ana", "", "").unwrap();
        db.add_transaction(id, Kind::Debt, 4200, "").unwrap();

        let data = build_encrypted_snapshot(&db, Some(&vault)).unwrap();
        assert!(is_encrypted(&data));
        assert!(!data.windows(3).any(|w| w == b"Ana"), "o nome do cliente não pode aparecer em claro");

        assert!(decrypt("senha-errada", &data).is_err());
        let plain = decrypt("senha-do-backup", &data).unwrap();
        assert!(plain.starts_with(b"SQLite format 3"));
    }

    #[test]
    fn restore_from_encrypted_backup_on_a_fresh_install() {
        // PC antigo: cria dados e um backup cifrado.
        let old = Db::open_in_memory().unwrap();
        let vault = security::create(&old, "senha-do-backup").unwrap();
        let id = old.add_customer("Ana", "", "").unwrap();
        old.add_transaction(id, Kind::Debt, 4200, "").unwrap();
        let data = build_encrypted_snapshot(&old, Some(&vault)).unwrap();

        // PC novo: banco vazio, sem nenhuma senha configurada.
        let dir = temp_dir("bk-fresh");
        let mut fresh = Db::open_in_memory().unwrap();
        assert!(matches!(restore(&mut fresh, &dir, data.clone(), None), Err(BackupError::PasswordRequired)));
        assert!(restore(&mut fresh, &dir, data.clone(), Some("errada")).is_err());
        restore(&mut fresh, &dir, data, Some("senha-do-backup")).unwrap();

        let customers = fresh.list_customers("").unwrap();
        assert_eq!(customers.len(), 1);
        assert_eq!(customers[0].balance_cents, 4200);
        // Cria um backup de segurança do estado anterior ao restaurar.
        assert!(list_local(&dir).iter().any(|b| b.name.ends_with("-antes-de-restaurar.db")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn session_key_opens_backups_made_with_the_same_password() {
        let db = Db::open_in_memory().unwrap();
        let vault = security::create(&db, "senha-do-backup").unwrap();
        let salt = current_salt(&db).unwrap();
        let data = build_encrypted_snapshot(&db, Some(&vault)).unwrap();

        // Mesmo sal: abre sem pedir a senha.
        assert!(open_with_session(data.clone(), Some(&vault), Some(&salt), None).is_ok());
        // Outro sal (senha trocada depois): exige a senha do momento do backup.
        let other_salt = [1u8; SALT_LEN];
        assert!(matches!(
            open_with_session(data.clone(), Some(&vault), Some(&other_salt), None),
            Err(BackupError::PasswordRequired)
        ));
        assert!(open_with_session(data, Some(&vault), Some(&other_salt), Some("senha-do-backup")).is_ok());
    }

    #[test]
    fn upload_requires_a_password() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(build_encrypted_snapshot(&db, None), Err(BackupError::NoVault)));
    }

    #[test]
    fn create_local_makes_a_valid_backup() {
        let dir = temp_dir("bk-local");
        let db = Db::open_in_memory().unwrap();
        db.add_customer("Ana", "", "").unwrap();
        let path = create_local(&db, &dir, LOCAL_KEEP).unwrap();
        assert!(Db::validate_backup(&path).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }
}
