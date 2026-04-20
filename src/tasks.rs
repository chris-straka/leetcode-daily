use crate::models::Data;
use chrono::Utc;
use poise::serenity_prelude as serenity;
use std::sync::Arc;

pub async fn schedule_daily_question(ctx: Arc<serenity::Context>, data: Arc<Data>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let today = Utc::now().format("%Y-%m-%d").to_string();
        
        let targets = {
            let db = data.db.read().await;
            db.iter()
                .filter(|(_, g)| g.active_daily && g.channel_id.is_some() && g.last_daily_date.as_ref() != Some(&today))
                .map(|(id, g)| (*id, g.channel_id.unwrap(), g.thread_id)).collect::<Vec<_>>()
        };

        if targets.is_empty() { continue; }
        let Ok(challenge) = crate::leetcode::fetch_daily_question().await else { continue; };

        for (guild_id, channel_id, old_thread_id) in targets {
            // This part now deletes the previous thread permanently
            if let Some(old_tid) = old_thread_id {
                let _ = old_tid.delete(&ctx).await;
            }

            let embed = crate::leetcode::create_embed(&challenge.question, &challenge.link);
            if let Ok(msg) = channel_id.send_message(&ctx, serenity::CreateMessage::new().content("Daily Question out!").embed(embed)).await {
                let tid = channel_id.create_thread_from_message(&ctx, msg.id, serenity::CreateThread::new(Utc::now().format("%d/%m/%Y").to_string())).await.map(|t| t.id).ok();
                if let Some(t) = tid { let _ = t.say(&ctx, "Paste solution in a code block to earn points!").await; }

                let mut db = data.db.write().await;
                if let Some(g) = db.get_mut(&guild_id) {
                    g.thread_id = tid;
                    g.last_daily_date = Some(today.clone());
                    for u in g.users.values_mut() { if u.submitted.is_none() { u.score = u.score.saturating_sub(1); } u.submitted = None; }
                }
                data.save_from_lock(&db).await;
            }
        }
    }
}

pub async fn schedule_contests(ctx: Arc<serenity::Context>, data: Arc<Data>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        let Ok(contests) = crate::leetcode::fetch_upcoming_contests().await else { continue; };
        let now = Utc::now().timestamp();

        for contest in contests {
            let diff = contest.start_time - now;
            
            let is_24h = diff > 86100 && diff <= 86400;
            let is_1h = diff > 3300 && diff <= 3600;
            let is_15m = diff > 600 && diff <= 900;
            let is_start = diff <= 0 && diff > -300; 

            let guilds = {
                let db = data.db.read().await;
                db.iter().filter(|(_, g)| g.active_weekly && g.weekly_id.is_some())
                    .map(|(id, g)| (*id, g.weekly_id.unwrap(), g.alerted_contests.clone())).collect::<Vec<_>>()
            };

            for (gid, cid, alerted) in guilds {
                let (k24, k1, k15, ks) = (
                    format!("{}-24h", contest.title), 
                    format!("{}-1h", contest.title), 
                    format!("{}-15m", contest.title), 
                    format!("{}-start", contest.title)
                );
                
                let mut key = None;
                let mut content = None;

                if is_24h && !alerted.contains(&k24) {
                    content = Some(format!("📅 **Contest Tomorrow**: {} starts in 24 hours! Get some sleep.", contest.title));
                    key = Some(k24);
                } else if is_1h && !alerted.contains(&k1) {
                    content = Some(format!("⏰ **1 Hour Warning**: {} is starting soon!", contest.title));
                    key = Some(k1);
                } else if is_15m && !alerted.contains(&k15) {
                    content = Some(format!("🚨 **15 Minutes**: {} is about to begin. Join the lobby!", contest.title));
                    key = Some(k15);
                } else if is_start && !alerted.contains(&ks) {
                    content = Some(format!("🚀 **Started**: {} is live! Good luck everyone!", contest.title));
                    key = Some(ks);
                }

                if let (Some(msg), Some(k)) = (content, key) {
                    let _ = cid.say(&ctx, msg).await;
                    let mut db = data.db.write().await;
                    if let Some(g) = db.get_mut(&gid) { 
                        g.alerted_contests.push(k); 
                        if g.alerted_contests.len() > 30 { g.alerted_contests.remove(0); }
                    }
                    data.save_from_lock(&db).await;
                }
            }
        }
    }
}