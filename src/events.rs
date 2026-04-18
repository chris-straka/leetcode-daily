use crate::models::{Data, Error};
use poise::serenity_prelude as serenity;
use regex::Regex;
use std::sync::LazyLock;

static CODE_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)```.+```").unwrap());

pub async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::Message { new_message: msg } => {
            if msg.author.bot || msg.guild_id.is_none() {
                return Ok(());
            }

            if msg.mentions_me(ctx).await.unwrap_or(false) {
                msg.reply(ctx, "👋 I'm awake! Use `/daily` for the link or `/scores` for the leaderboard.").await?;
                return Ok(());
            }

            if CODE_BLOCK_RE.is_match(&msg.content) {
                let guild_id = msg.guild_id.unwrap();
                
                let (is_thread, username_opt) = {
                    let db = data.db.read().await;
                    if let Some(g) = db.get(&guild_id) {
                        let is_th = g.active_daily && Some(msg.channel_id) == g.thread_id;
                        let uname = g.users.get(&msg.author.id).and_then(|u| u.leetcode_username.clone());
                        (is_th, uname)
                    } else {
                        (false, None)
                    }
                };

                if !is_thread {
                    return Ok(());
                }

                let Some(username) = username_opt else {
                    msg.reply(ctx, "❌ Please run `/register <your_leetcode_username>` first!").await?;
                    return Ok(());
                };

                let daily = match crate::leetcode::fetch_daily_question().await {
                    Ok(d) => d,
                    Err(_) => {
                        msg.reply(ctx, "Error contacting LeetCode API.").await?;
                        return Ok(());
                    }
                };

                let daily_slug = daily.link.trim_matches('/').split('/').last().unwrap_or_default();

                let subs = match crate::leetcode::fetch_recent_ac_submissions(&username).await {
                    Ok(s) => s,
                    Err(_) => {
                        msg.reply(ctx, "Error fetching your profile. Is it public?").await?;
                        return Ok(());
                    }
                };

                let is_accepted = subs.iter().any(|sub| sub.title_slug == daily_slug);

                if !is_accepted {
                    msg.reply(ctx, "❌ Couldn't find an Accepted submission for today's daily! (Wait a few seconds after submitting to LeetCode).").await?;
                    return Ok(());
                }

                let mut db = data.db.write().await;
                let guild_data = db.entry(guild_id).or_default();

                let solvers_so_far = guild_data.users.values().filter(|u| u.submitted.is_some()).count();
                let user = guild_data.users.entry(msg.author.id).or_default();
                
                if user.submitted.is_none() {
                    let base_score = match daily.question.difficulty.as_str() {
                        "Easy" => 1,
                        "Medium" => 2,
                        "Hard" => 3,
                        _ => 1,
                    };

                    let total_gain = base_score;
                    user.submitted = Some(msg.link());
                    user.monthly_record += 1;
                    user.score += total_gain;
                    user.days_missed = 0;

                    if solvers_so_far == 0 {
                        if let Some(main_channel) = guild_data.channel_id {
                            let announcement = format!(
                                "🥇 **<@{}>** is the first to solve today's daily!",
                                msg.author.id
                            );
                            let _ = main_channel.say(&ctx.http, announcement).await;
                        }
                    }

                    let response = format!("✅ Verified via API! +**{}** pts.", total_gain);

                    msg.reply(ctx, response).await?;
                    data.save_from_lock(&db).await;
                }
            }
        }
        _ => {}
    }
    Ok(())
}