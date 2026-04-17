use crate::models::Data;
use chrono::Utc;
use poise::serenity_prelude as serenity;
use std::sync::Arc;

pub async fn schedule_daily_question(ctx: Arc<serenity::Context>, data: Arc<Data>) {
    loop {
        // Sleep for 60 seconds, then check if we need to do anything
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;

        let today_str = Utc::now().format("%Y-%m-%d").to_string();
        let mut targets = Vec::new();

        {
            let db = data.db.read().await;
            for (guild_id, guild_data) in db.iter() {
                if guild_data.active_daily && guild_data.channel_id.is_some() {
                    // Check our new state field
                    if guild_data.last_daily_date.as_ref() != Some(&today_str) {
                        targets.push((*guild_id, guild_data.channel_id.unwrap()));
                    }
                }
            }
        }

        if targets.is_empty() {
            continue; // Nothing to do, go back to sleep
        }

        let challenge = match crate::leetcode::fetch_daily_question().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("LeetCode API error: {e}");
                continue;
            }
        };

        for (guild_id, channel_id) in targets {
            let embed = crate::leetcode::create_embed(&challenge.question, &challenge.link);

            if let Ok(msg) = channel_id
                .send_message(
                    &ctx,
                    serenity::CreateMessage::new()
                        .content("Daily Question out!")
                        .embed(embed),
                )
                .await
            {
                let thread_name = Utc::now().format("%d/%m/%Y").to_string();
                let thread_id = channel_id
                    .create_thread_from_message(
                        &ctx,
                        msg.id,
                        serenity::CreateThread::new(thread_name)
                            .kind(serenity::ChannelType::PublicThread),
                    )
                    .await
                    .map(|t| t.id)
                    .ok();

                if let Some(tid) = thread_id {
                    let _ = tid
                        .say(
                            &ctx,
                            "Paste your solution in a code block here to earn points!",
                        )
                        .await;
                }

                // Update the state so we don't post again today
                let mut db = data.db.write().await;
                if let Some(guild_data) = db.get_mut(&guild_id) {
                    guild_data.thread_id = thread_id;
                    guild_data.last_daily_date = Some(today_str.clone());

                    for user in guild_data.users.values_mut() {
                        if user.submitted.is_none() {
                            user.days_missed += 1;
                            user.score = user.score.saturating_sub(1);
                        }
                        user.submitted = None;
                        user.voted_for = None;
                    }
                }
            }
        }
        data.save().await;
    }
}

pub async fn schedule_contests(ctx: Arc<serenity::Context>, data: Arc<Data>) {
    loop {
        // Poll every 5 minutes for contests
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;

        let Ok(contests) = crate::leetcode::fetch_upcoming_contests().await else {
            continue;
        };

        let now = Utc::now().timestamp();
        let mut needs_save = false;

        for contest in contests {
            let starts_in = contest.start_time - now;

            // Check if within 15 minutes
            let is_15m_warning = starts_in > 0 && starts_in <= 900;
            // Check if currently active (within 2 hours of start)
            let is_start_warning = starts_in <= 0 && starts_in > -7200;

            let key_15m = format!("{}-15m", contest.title);
            let key_start = format!("{}-start", contest.title);

            let guilds = { data.db.read().await.clone() };

            for (guild_id, guild_data) in guilds {
                if !guild_data.active_weekly || guild_data.weekly_id.is_none() {
                    continue;
                }

                let channel_id = guild_data.weekly_id.unwrap();
                let mut db = data.db.write().await;
                let g_data = db.get_mut(&guild_id).unwrap();

                if is_15m_warning && !g_data.alerted_contests.contains(&key_15m) {
                    let msg = format!(
                        "🚨 **15 MINUTE WARNING** 🚨\n**{}** is starting soon! Get ready on LeetCode.",
                        contest.title
                    );
                    let _ = channel_id.say(&ctx, msg).await;
                    g_data.alerted_contests.push(key_15m.clone());
                    needs_save = true;
                }

                if is_start_warning && !g_data.alerted_contests.contains(&key_start) {
                    let msg = format!("🚀 **{} HAS STARTED!** Good luck!", contest.title);
                    let _ = channel_id.say(&ctx, msg).await;
                    g_data.alerted_contests.push(key_start.clone());
                    needs_save = true;
                }

                // Prevent the tracking array from growing infinitely
                if g_data.alerted_contests.len() > 20 {
                    let len = g_data.alerted_contests.len();
                    g_data.alerted_contests.drain(0..(len - 20));
                }
            }
        }

        if needs_save {
            data.save().await;
        }
    }
}
