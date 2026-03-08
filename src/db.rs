use rusqlite::{Connection, Result};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
            )",
            [],
        )?;
        Ok(Self { conn })
    }

    pub fn save_memo(&self, content: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO memos (content) VALUES (?1)",
            [content],
        )?;
        Ok(())
    }

    pub fn get_recent_memos(&self, limit: usize) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT content, created_at FROM memos ORDER BY created_at DESC LIMIT ?1",
        )?;
        let memos = stmt
            .query_map([limit as i64], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>>>()?;
        Ok(memos)
    }
}
