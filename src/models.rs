use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

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

#[derive(Debug, Clone)]
pub struct Data {
    pub db: Arc<tokio::sync::RwLock<HashMap<serenity::GuildId, GuildData>>>,
}

impl Data {
    pub async fn save(&self) {
        let db = self.db.read().await;
        self.save_from_lock(&db).await;
    }

    pub async fn save_from_lock(&self, db: &HashMap<serenity::GuildId, GuildData>) {
        if let Ok(json) = serde_json::to_string_pretty(db) {
            let _ = tokio::fs::write("database.json", json).await;
        }
    }
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;