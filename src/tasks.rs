use crate::models::{Data, Error};
use chrono::Utc;
use poise::serenity_prelude as serenity;
use std::sync::Arc;

pub async fn schedule_catchup(ctx: Arc<serenity::Context>, data: Arc<Data>) {
    loop {
        do_catchup(&ctx, &data).await;
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
    }
}

pub async fn do_catchup(ctx: &serenity::Context, data: &Data) {
    let guilds = {
        let db = data.db.read().await;
        db.clone()
    };

    for (guild_id, g) in guilds {
        if g.active_leetcode {
            if let Some(tid) = g.thread_id {
                let _ = catchup_thread(ctx, data, tid, guild_id).await;
            }
        }
        if g.active_neetcode {
            if let Some(tid) = g.neetcode_thread_id {
                let _ = catchup_thread(ctx, data, tid, guild_id).await;
            }
        }
    }
}

async fn catchup_thread(
    ctx: &serenity::Context,
    data: &Data,
    thread_id: serenity::ChannelId,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    if let Ok(mut messages) = thread_id
        .messages(ctx, serenity::GetMessages::new().limit(100))
        .await
    {
        messages.sort_by_key(|m| m.timestamp);
        for msg in messages {
            if msg.author.bot {
                continue;
            }
            let _ = crate::events::process_solution_message(ctx, &msg, data, guild_id).await;
        }
    }
    Ok(())
}

pub async fn schedule_monthly_winner(ctx: Arc<serenity::Context>, data: Arc<Data>) {
    loop {
        let now = chrono::Utc::now();
        use chrono::Datelike;
        let current_month = now.month();

        let mut changed = false;
        {
            let mut db = data.db.write().await;
            for (_, g) in db.iter_mut() {
                if g.last_processed_month.is_none() {
                    g.last_processed_month = Some(current_month);
                    changed = true;
                    continue;
                }

                if let Some(last_month) = g.last_processed_month {
                    if last_month != current_month {
                        let prev_month_name =
                            crate::scoring::finished_month_label(last_month, now.date_naive());
                        let (best_score, best_users) = crate::scoring::monthly_winners(g);

                        g.monthly_winners.push(crate::models::MonthlyWinner {
                            month_year: prev_month_name.clone(),
                            user_ids: best_users.clone(),
                            score: best_score,
                        });

                        if let Some(cid) = g.channel_id {
                            let msg = if !best_users.is_empty() {
                                let users_str = best_users
                                    .iter()
                                    .map(|id| format!("<@{}>", id))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                format!(
                                    "🏆 **Leetcoder of the Month** for {} is {} with **{}** points! Scores have been reset.",
                                    prev_month_name, users_str, best_score
                                )
                            } else {
                                format!(
                                    "🏆 No one scored points in {}! Scores have been reset.",
                                    prev_month_name
                                )
                            };
                            let _ = cid.say(&ctx.http, msg).await;
                        }

                        for status in g.users.values_mut() {
                            status.score = 0;
                            status.monthly_record = 0;
                        }

                        g.last_processed_month = Some(current_month);
                        changed = true;
                    }
                }
            }
            if changed {
                data.save_from_lock(&db).await;
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

pub async fn schedule_daily_question(ctx: Arc<serenity::Context>, data: Arc<Data>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let today = Utc::now().format("%Y-%m-%d").to_string();

        let targets = {
            let db = data.db.read().await;
            db.iter()
                .filter(|(_, g)| {
                    g.active_leetcode
                        && g.channel_id.is_some()
                        && g.last_daily_date.as_ref() != Some(&today)
                })
                .map(|(id, g)| (*id, g.channel_id.unwrap(), g.thread_id))
                .collect::<Vec<_>>()
        };

        if targets.is_empty() {
            continue;
        }
        let Ok(challenge) = crate::leetcode::fetch_daily_question().await else {
            continue;
        };

        for (guild_id, channel_id, old_thread_id) in targets {
            if let Some(old_tid) = old_thread_id {
                let _ = catchup_thread(&ctx, &data, old_tid, guild_id).await;
                let _ = old_tid.delete(&ctx).await;
            }

            let embed = crate::leetcode::create_embed(&challenge.question, &challenge.link);
            if let Ok(msg) = channel_id
                .send_message(
                    &ctx,
                    serenity::CreateMessage::new()
                        .content("LeetCode Daily is out!")
                        .embed(embed),
                )
                .await
            {
                let tid = channel_id
                    .create_thread_from_message(
                        &ctx,
                        msg.id,
                        serenity::CreateThread::new(format!(
                            "LeetCode - {}",
                            Utc::now().format("%d/%m/%Y")
                        )),
                    )
                    .await
                    .map(|t| t.id)
                    .ok();
                if let Some(t) = tid {
                    let _ = t
                        .say(
                            &ctx,
                            "Run `/claim` to verify your solution and earn points!",
                        )
                        .await;
                }

                let mut db = data.db.write().await;
                if let Some(g) = db.get_mut(&guild_id) {
                    g.thread_id = tid;
                    g.last_daily_date = Some(today.clone());
                    g.last_daily_slug = Some(challenge.question.title_slug.clone());
                    g.last_daily_diff = Some(challenge.question.difficulty.clone());
                    crate::scoring::daily_rollover(g, crate::scoring::Daily::LeetCode);
                }
                data.save_from_lock(&db).await;
            }
        }
    }
}

pub async fn schedule_neetcode_daily(ctx: Arc<serenity::Context>, data: Arc<Data>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let today = Utc::now().format("%Y-%m-%d").to_string();

        let targets = {
            let db = data.db.read().await;
            db.iter()
                .filter(|(_, g)| {
                    g.active_neetcode
                        && g.channel_id.is_some()
                        && g.last_neetcode_date.as_ref() != Some(&today)
                })
                .map(|(id, g)| (*id, g.channel_id.unwrap(), g.neetcode_thread_id))
                .collect::<Vec<_>>()
        };

        if targets.is_empty() {
            continue;
        }

        let slug = crate::scoring::neetcode_slug_for(Utc::now().date_naive());

        let Ok(question) = crate::leetcode::fetch_question_by_slug(slug).await else {
            continue;
        };
        let link = format!("/problems/{}/", slug);

        for (guild_id, channel_id, old_thread_id) in targets {
            if let Some(old_tid) = old_thread_id {
                let _ = catchup_thread(&ctx, &data, old_tid, guild_id).await;
                let _ = old_tid.delete(&ctx).await;
            }

            let embed = crate::leetcode::create_embed(&question, &link);
            if let Ok(msg) = channel_id
                .send_message(
                    &ctx,
                    serenity::CreateMessage::new()
                        .content("NeetCode 250 Daily is out!")
                        .embed(embed),
                )
                .await
            {
                let tid = channel_id
                    .create_thread_from_message(
                        &ctx,
                        msg.id,
                        serenity::CreateThread::new(format!(
                            "NeetCode - {}",
                            Utc::now().format("%d/%m/%Y")
                        )),
                    )
                    .await
                    .map(|t| t.id)
                    .ok();
                if let Some(t) = tid {
                    let _ = t
                        .say(
                            &ctx,
                            "Run `/claim` to verify your solution and earn points!",
                        )
                        .await;
                }

                let mut db = data.db.write().await;
                if let Some(g) = db.get_mut(&guild_id) {
                    g.neetcode_thread_id = tid;
                    g.last_neetcode_date = Some(today.clone());
                    g.last_neetcode_slug = Some(slug.to_string());
                    g.last_neetcode_diff = Some(question.difficulty.clone());
                    crate::scoring::daily_rollover(g, crate::scoring::Daily::NeetCode);
                }
                data.save_from_lock(&db).await;
            }
        }
    }
}

pub async fn schedule_contests(ctx: Arc<serenity::Context>, data: Arc<Data>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        let Ok(contests) = crate::leetcode::fetch_upcoming_contests().await else {
            continue;
        };
        let now = Utc::now().timestamp();

        for contest in contests {
            let Some(stage) = crate::scoring::contest_stage(contest.start_time - now) else {
                continue;
            };
            let key = stage.key(&contest.title);

            let guilds = {
                let db = data.db.read().await;
                db.iter()
                    .filter(|(_, g)| g.active_weekly && g.weekly_id.is_some())
                    .filter(|(_, g)| !g.alerted_contests.contains(&key))
                    .map(|(id, g)| (*id, g.weekly_id.unwrap()))
                    .collect::<Vec<_>>()
            };

            for (gid, cid) in guilds {
                let _ = cid.say(&ctx, stage.message(&contest.title)).await;
                let mut db = data.db.write().await;
                if let Some(g) = db.get_mut(&gid) {
                    g.alerted_contests.push(key.clone());
                    if g.alerted_contests.len() > 30 {
                        g.alerted_contests.remove(0);
                    }
                }
                data.save_from_lock(&db).await;
            }
        }
    }
}
