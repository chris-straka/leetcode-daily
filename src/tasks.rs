use crate::models::Data;
use chrono::{TimeZone, Utc};
use poise::serenity_prelude as serenity;
use std::sync::Arc;

pub async fn schedule_daily_question(ctx: Arc<serenity::Context>, data: Arc<Data>) {
    loop {
        let now = Utc::now();
        let tomorrow = Utc.from_utc_datetime(&now.date_naive().succ_opt().expect("date").and_hms_opt(0, 1, 0).expect("time"));
        let wait = (tomorrow - now).to_std().unwrap_or(std::time::Duration::from_secs(60));

        println!("Waiting {:?} for daily update...", wait);
        tokio::time::sleep(wait).await;

        let challenge = match crate::leetcode::fetch_daily_question().await {
            Ok(c) => c,
            Err(e) => { eprintln!("LeetCode API error: {e}"); tokio::time::sleep(std::time::Duration::from_secs(60)).await; continue; }
        };

        // Get guild list first to avoid holding a global lock while talking to Discord
        let guild_ids: Vec<_> = { data.db.read().await.keys().cloned().collect() };

        for guild_id in guild_ids {
            let mut db = data.db.write().await;
            let Some(guild_data) = db.get_mut(&guild_id) else { continue; };
            if !guild_data.active_daily || guild_data.channel_id.is_none() { continue; }

            let channel_id = guild_data.channel_id.unwrap();
            for user in guild_data.users.values_mut() {
                if user.submitted.is_none() { user.days_missed += 1; user.score = user.score.saturating_sub(1); }
                user.submitted = None; user.voted_for = None;
            }

            let embed = crate::leetcode::create_embed(&challenge.question, &challenge.link);
            if let Ok(msg) = channel_id.send_message(&ctx, serenity::CreateMessage::new().content("Daily Question out!").embed(embed)).await {
                let thread_name = Utc::now().format("%d/%m/%Y").to_string();
                if let Ok(thread) = channel_id.create_thread_from_message(&ctx, msg.id, serenity::CreateThread::new(thread_name).kind(serenity::ChannelType::PublicThread)).await {
                    guild_data.thread_id = Some(thread.id);
                    let _ = thread.id.say(&ctx, "Paste your solution in a code block here to earn points!").await;
                }
            }
        }
        data.save().await;
    }
}