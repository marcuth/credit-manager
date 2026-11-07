use rusqlite::{Connection, params};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("erro no banco de dados: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;

#[derive(Clone, Debug)]
pub struct Customer {
    pub id: i64,
    pub name: String,
    pub phone: String,
    pub email: String,
    /// Positivo = cliente deve. Negativo = cliente tem crédito.
    pub balance_cents: i64,
    /// Preenchido quando o cliente vincula o Telegram pelo link do bot.
    pub telegram_chat_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Transaction {
    pub id: i64,
    pub kind: Kind,
    pub amount_cents: i64,
    pub note: String,
    pub created_at: String,
}

/// Transação já com o nome do cliente (para a tela "Transações").
#[derive(Clone, Debug)]
pub struct TransactionEntry {
    pub customer_name: String,
    pub tx: Transaction,
}

#[derive(Clone, Debug)]
pub struct Charge {
    pub id: i64,
    pub customer_id: i64,
    pub customer_name: String,
    pub amount_cents: i64,
    pub url: String,
    pub paid: bool,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Debt,
    Payment,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Debt => "debt",
            Kind::Payment => "payment",
        }
    }
}

fn random_code() -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    (0..2)
        .map(|_| {
            let mut h = RandomState::new().build_hasher();
            h.write_u8(0);
            format!("{:016x}", h.finish())
        })
        .collect()
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE customers (
                     id         INTEGER PRIMARY KEY,
                     name       TEXT NOT NULL,
                     phone      TEXT NOT NULL DEFAULT '',
                     email      TEXT NOT NULL DEFAULT '',
                     archived   INTEGER NOT NULL DEFAULT 0,
                     created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
                 );
                 CREATE TABLE transactions (
                     id           INTEGER PRIMARY KEY,
                     customer_id  INTEGER NOT NULL REFERENCES customers(id),
                     kind         TEXT NOT NULL CHECK (kind IN ('debt','payment')),
                     amount_cents INTEGER NOT NULL CHECK (amount_cents > 0),
                     note         TEXT NOT NULL DEFAULT '',
                     source       TEXT NOT NULL DEFAULT 'manual',
                     external_id  TEXT UNIQUE,
                     created_at   TEXT NOT NULL DEFAULT (datetime('now','localtime'))
                 );
                 CREATE INDEX idx_tx_customer ON transactions(customer_id, id DESC);
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        if version < 2 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE charges (
                     id           INTEGER PRIMARY KEY,
                     customer_id  INTEGER NOT NULL REFERENCES customers(id),
                     amount_cents INTEGER NOT NULL CHECK (amount_cents > 0),
                     external_ref TEXT NOT NULL UNIQUE,
                     url          TEXT NOT NULL,
                     status       TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','paid')),
                     created_at   TEXT NOT NULL DEFAULT (datetime('now','localtime'))
                 );
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }
        if version < 3 {
            self.conn.execute_batch(
                "BEGIN;
                 ALTER TABLE customers ADD COLUMN telegram_chat_id TEXT;
                 ALTER TABLE customers ADD COLUMN telegram_link_code TEXT;
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }
        Ok(())
    }

    pub fn add_customer(&self, name: &str, phone: &str, email: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO customers (name, phone, email) VALUES (?1, ?2, ?3)",
            params![name.trim(), phone.trim(), email.trim()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_customers(&self, search: &str) -> Result<Vec<Customer>> {
        let like = format!("%{}%", search.trim());
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.name, c.phone, c.email,
                    COALESCE(SUM(CASE t.kind WHEN 'debt' THEN t.amount_cents
                                             ELSE -t.amount_cents END), 0) AS balance,
                    c.telegram_chat_id
             FROM customers c LEFT JOIN transactions t ON t.customer_id = c.id
             WHERE c.archived = 0 AND c.name LIKE ?1
             GROUP BY c.id
             ORDER BY balance DESC, c.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([like], |r| {
            Ok(Customer {
                id: r.get(0)?,
                name: r.get(1)?,
                phone: r.get(2)?,
                email: r.get(3)?,
                balance_cents: r.get(4)?,
                telegram_chat_id: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn add_transaction(&self, customer_id: i64, kind: Kind, cents: i64, note: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO transactions (customer_id, kind, amount_cents, note) VALUES (?1,?2,?3,?4)",
            params![customer_id, kind.as_str(), cents, note.trim()],
        )?;
        Ok(())
    }

    /// Código aleatório e imprevisível que o cliente envia ao bot (`/start <código>`)
    /// para vincular o Telegram dele. Não usamos o id do cliente para ninguém
    /// poder se passar por outro cliente.
    pub fn ensure_telegram_link_code(&self, customer_id: i64) -> Result<String> {
        let existing: Option<String> = self.conn.query_row(
            "SELECT telegram_link_code FROM customers WHERE id = ?1",
            [customer_id],
            |r| r.get(0),
        )?;
        if let Some(code) = existing {
            return Ok(code);
        }
        let code = random_code();
        self.conn.execute(
            "UPDATE customers SET telegram_link_code = ?2 WHERE id = ?1",
            params![customer_id, code],
        )?;
        Ok(code)
    }

    /// Vincula o chat do Telegram ao cliente dono do código. `false` se o código não existe.
    pub fn link_telegram(&self, code: &str, chat_id: i64) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE customers SET telegram_chat_id = ?2 WHERE telegram_link_code = ?1",
            params![code, chat_id.to_string()],
        )?;
        Ok(n > 0)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        Ok(rows.next()?.map(|r| r.get(0)).transpose()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn add_charge(&self, customer_id: i64, cents: i64, external_ref: &str, url: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO charges (customer_id, amount_cents, external_ref, url) VALUES (?1,?2,?3,?4)",
            params![customer_id, cents, external_ref, url],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_charges(&self) -> Result<Vec<Charge>> {
        let mut stmt = self.conn.prepare(
            "SELECT ch.id, ch.customer_id, c.name, ch.amount_cents, ch.url, ch.status, ch.created_at
             FROM charges ch JOIN customers c ON c.id = ch.customer_id ORDER BY ch.id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Charge {
                id: r.get(0)?,
                customer_id: r.get(1)?,
                customer_name: r.get(2)?,
                amount_cents: r.get(3)?,
                url: r.get(4)?,
                paid: r.get::<_, String>(5)? == "paid",
                created_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// (id da cobrança, referência externa) das cobranças ainda não pagas.
    pub fn pending_charges(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, external_ref FROM charges WHERE status = 'pending'")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Marca a cobrança como paga e registra o pagamento, tudo ou nada.
    /// Idempotente: devolve `false` se já estava paga ou se o pagamento já foi registrado.
    pub fn mark_charge_paid(&self, charge_id: i64, payment_id: &str) -> Result<bool> {
        let tx = self.conn.unchecked_transaction()?;
        let updated = tx.execute(
            "UPDATE charges SET status = 'paid' WHERE id = ?1 AND status = 'pending'",
            [charge_id],
        )?;
        if updated == 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT OR IGNORE INTO transactions (customer_id, kind, amount_cents, note, source, external_id)
             SELECT customer_id, 'payment', amount_cents, 'Pago pelo link de cobrança', 'mercadopago', ?2
             FROM charges WHERE id = ?1",
            params![charge_id, payment_id],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn list_all_transactions(&self, limit: i64) -> Result<Vec<TransactionEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.kind, t.amount_cents, t.note, t.created_at, c.name
             FROM transactions t JOIN customers c ON c.id = t.customer_id
             ORDER BY t.id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            let kind: String = r.get(1)?;
            Ok(TransactionEntry {
                customer_name: r.get(5)?,
                tx: Transaction {
                    id: r.get(0)?,
                    kind: if kind == "debt" { Kind::Debt } else { Kind::Payment },
                    amount_cents: r.get(2)?,
                    note: r.get(3)?,
                    created_at: r.get(4)?,
                },
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn list_transactions(&self, customer_id: i64) -> Result<Vec<Transaction>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, amount_cents, note, created_at
             FROM transactions WHERE customer_id = ?1 ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([customer_id], |r| {
            let kind: String = r.get(1)?;
            Ok(Transaction {
                id: r.get(0)?,
                kind: if kind == "debt" { Kind::Debt } else { Kind::Payment },
                amount_cents: r.get(2)?,
                note: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_follows_transactions() {
        let db = Db::open_in_memory().unwrap();
        let id = db.add_customer("Dona Maria", "", "").unwrap();
        db.add_transaction(id, Kind::Debt, 5000, "arroz").unwrap();
        db.add_transaction(id, Kind::Payment, 1500, "").unwrap();
        let c = &db.list_customers("").unwrap()[0];
        assert_eq!(c.balance_cents, 3500);
        assert_eq!(db.list_transactions(id).unwrap().len(), 2);
    }

    #[test]
    fn paid_charge_registers_payment_once() {
        let db = Db::open_in_memory().unwrap();
        let id = db.add_customer("Seu Zé", "", "").unwrap();
        db.add_transaction(id, Kind::Debt, 5000, "").unwrap();
        let ch = db.add_charge(id, 5000, "ref-1", "http://x").unwrap();
        assert!(db.mark_charge_paid(ch, "pay-1").unwrap());
        assert!(!db.mark_charge_paid(ch, "pay-1").unwrap());
        assert_eq!(db.list_customers("").unwrap()[0].balance_cents, 0);
        assert!(db.pending_charges().unwrap().is_empty());
    }

    #[test]
    fn telegram_link_flow() {
        let db = Db::open_in_memory().unwrap();
        let id = db.add_customer("Ana", "", "").unwrap();
        let code = db.ensure_telegram_link_code(id).unwrap();
        assert_eq!(code.len(), 32);
        assert_eq!(code, db.ensure_telegram_link_code(id).unwrap());
        assert!(!db.link_telegram("errado", 42).unwrap());
        assert!(db.link_telegram(&code, 42).unwrap());
        assert_eq!(db.list_customers("").unwrap()[0].telegram_chat_id.as_deref(), Some("42"));
    }

    #[test]
    fn settings_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.get_setting("a").unwrap(), None);
        db.set_setting("a", "1").unwrap();
        db.set_setting("a", "2").unwrap();
        assert_eq!(db.get_setting("a").unwrap().as_deref(), Some("2"));
    }

    #[test]
    fn rejects_non_positive_amount() {
        let db = Db::open_in_memory().unwrap();
        let id = db.add_customer("X", "", "").unwrap();
        assert!(db.add_transaction(id, Kind::Debt, 0, "").is_err());
    }
}
