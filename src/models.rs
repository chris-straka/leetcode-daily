use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct MonthlyWinner {
    pub month_year: String,
    pub user_ids: Vec<serenity::UserId>,
    pub score: usize,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct GuildData {
    pub users: HashMap<serenity::UserId, Status>,
    pub channel_id: Option<serenity::ChannelId>,
    pub thread_id: Option<serenity::ChannelId>,
    pub neetcode_thread_id: Option<serenity::ChannelId>,
    pub weekly_id: Option<serenity::ChannelId>,
    pub active_weekly: bool,
    #[serde(alias = "active_daily")]
    pub active_leetcode: bool,
    pub active_neetcode: bool,
    pub last_daily_date: Option<String>, 
    pub last_neetcode_date: Option<String>,
    pub last_daily_slug: Option<String>,
    pub last_daily_diff: Option<String>,
    pub last_neetcode_slug: Option<String>,
    pub last_neetcode_diff: Option<String>,
    pub alerted_contests: Vec<String>,
    pub last_processed_month: Option<u32>,
    pub monthly_winners: Vec<MonthlyWinner>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Status {
    pub leetcode_username: Option<String>,
    pub submitted: Option<String>,
    pub nc_submitted: Option<String>,
    pub weekly_submissions: usize,
    pub monthly_record: u32,
    pub days_missed: u32,
    pub score: usize,
    pub contest_rating: f64,
}

/// Every guild's state, persisted as one JSON file.
pub type Db = HashMap<serenity::GuildId, GuildData>;

pub const DB_PATH: &str = "database.json";

/// Loads the database. A missing file is a fresh install (empty DB). A file
/// that exists but doesn't parse is an error: starting empty would overwrite
/// every guild's scores on the next save.
pub async fn load_db(path: &Path) -> Result<Db, Error> {
    match tokio::fs::read_to_string(path).await {
        Ok(json) => serde_json::from_str(&json)
            .map_err(|e| format!("{} is corrupt ({e}); refusing to start over it", path.display()).into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Db::new()),
        Err(e) => Err(e.into()),
    }
}

/// Writes the database atomically: a temp file in the same directory, then a
/// rename over the old file. A crash mid-write leaves the previous version
/// intact instead of a truncated file.
pub async fn save_db(path: &Path, db: &Db) -> Result<(), Error> {
    let json = serde_json::to_string_pretty(db)?;
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, json).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Data {
    pub db: Arc<tokio::sync::RwLock<Db>>,
    // In-memory set to prevent concurrent checks for the same user
    pub processing: Arc<Mutex<HashSet<(serenity::GuildId, serenity::UserId, String)>>>,
}

impl Data {
    pub async fn save(&self) {
        let db = self.db.read().await;
        self.save_from_lock(&db).await;
    }

    pub async fn save_from_lock(&self, db: &Db) {
        if let Err(e) = save_db(Path::new(DB_PATH), db).await {
            tracing::error!("Failed to save {DB_PATH}: {e}");
        }
    }
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("leetcode-daily-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("database.json")
    }

    #[tokio::test]
    async fn round_trips_through_disk() {
        let path = temp_path("roundtrip");
        let mut db = Db::new();
        let g = db.entry(serenity::GuildId::new(7)).or_default();
        g.users.entry(serenity::UserId::new(1)).or_default().score = 9;
        save_db(&path, &db).await.unwrap();

        let loaded = load_db(&path).await.unwrap();
        assert_eq!(loaded[&serenity::GuildId::new(7)].users[&serenity::UserId::new(1)].score, 9);
        assert!(!path.with_extension("json.tmp").exists(), "temp file renamed away");
    }

    #[tokio::test]
    async fn missing_file_is_an_empty_db() {
        let path = temp_path("missing").with_file_name("nope.json");
        assert!(load_db(&path).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn corrupt_file_is_an_error_not_an_empty_db() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "{\"123\": {\"users\": ").unwrap(); // truncated mid-write
        assert!(load_db(&path).await.is_err());
    }

    #[tokio::test]
    async fn the_checked_out_database_loads_if_present() {
        // database.json is gitignored live data; when it's there, prove the
        // strict loader accepts it so a deploy can't refuse to start.
        let path = Path::new(DB_PATH);
        if path.exists() {
            load_db(path).await.unwrap();
        }
    }

    #[test]
    fn old_files_with_active_daily_still_load() {
        let db: Db = serde_json::from_str(r#"{"42": {"active_daily": true}}"#).unwrap();
        assert!(db[&serenity::GuildId::new(42)].active_leetcode);
    }
}
