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

            // 1. Respond to bot pings
            if msg.mentions_me(ctx).await.unwrap_or(false) {
                msg.reply(ctx, "👋 I'm awake! Use `/daily` for the link or `/scores` for the leaderboard.").await?;
                return Ok(());
            }

            // 2. Handle Code Block Verification
            if CODE_BLOCK_RE.is_match(&msg.content) {
                let username_opt = {
                    let db = data.db.read().await;
                    db.get(&msg.guild_id.unwrap())
                        .and_then(|g| g.users.get(&msg.author.id))
                        .and_then(|u| u.leetcode_username.clone())
                };

                let Some(username) = username_opt else {
                    msg.reply(ctx, "❌ Please run `/register <your_leetcode_username>` first!").await?;
                    return Ok(());
                };

                // Fetch Daily Question data
                let daily = match crate::leetcode::fetch_daily_question().await {
                    Ok(d) => d,
                    Err(_) => {
                        msg.reply(ctx, "Error contacting LeetCode API.").await?;
                        return Ok(());
                    }
                };

                let daily_slug = daily.link.trim_matches('/').split('/').last().unwrap_or_default();

                // Fetch user's recent submissions
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

                // 3. Success Logic & Scoring
                let mut db = data.db.write().await;
                let guild_id = msg.guild_id.unwrap();
                let guild_data = db.entry(guild_id).or_default();

                // Ensure message is in the correct thread
                if guild_data.active_daily && Some(msg.channel_id) == guild_data.thread_id {
                    let solvers_so_far = guild_data.users.values().filter(|u| u.submitted.is_some()).count();
                    let user = guild_data.users.entry(msg.author.id).or_default();
                    
                    if user.submitted.is_none() {
                        let base_score = match daily.question.difficulty.as_str() {
                            "Easy" => 1,
                            "Medium" => 2,
                            "Hard" => 3,
                            _ => 1,
                        };

                        let bonus = match solvers_so_far {
                            0 => 2, // 1st gets +2
                            1 => 1, // 2nd gets +1
                            _ => 0,
                        };

                        let total_gain = base_score + bonus;
                        user.submitted = Some(msg.link());
                        user.monthly_record += 1;
                        user.score += total_gain;
                        user.days_missed = 0;

                        // --- NEW: Announcement to the main channel ---
                        if bonus > 0 {
                            if let Some(main_channel) = guild_data.channel_id {
                                let medal = if solvers_so_far == 0 { "🥇" } else { "🥈" };
                                let placement = if solvers_so_far == 0 { "1st" } else { "2nd" };
                                let announcement = format!(
                                    "{} **<@{}>** is the **{}** solver! They earned a +{} point speed bonus!",
                                    medal, msg.author.id, placement, bonus
                                );
                                // Send to the main guild channel
                                let _ = main_channel.say(&ctx.http, announcement).await;
                            }
                        }

                        let mut response = format!("✅ Verified via API! +**{}** pts.", total_gain);
                        if bonus > 0 {
                            response.push_str(&format!(" (Bonus for being solver #{}!)", solvers_so_far + 1));
                        }

                        msg.reply(ctx, response).await?;
                        data.save_from_lock(&db).await;
                    }
                }
            }
        }

        serenity::FullEvent::InteractionCreate { interaction } => {
            if let serenity::Interaction::Component(component) = interaction {
                if component.data.custom_id == "favourite_submission" {
                    if let serenity::ComponentInteractionDataKind::UserSelect { values } = &component.data.kind {
                        let voted_for = values[0];
                        
                        if voted_for == component.user.id {
                            component.create_response(ctx, serenity::CreateInteractionResponse::Message(
                                serenity::CreateInteractionResponseMessage::new().content("Self-voting is cringe.").ephemeral(true),
                            )).await?;
                            return Ok(());
                        }

                        let mut db = data.db.write().await;
                        let user = db.entry(component.guild_id.unwrap()).or_default().users.entry(component.user.id).or_default();
                        user.voted_for = Some(voted_for);
                        data.save_from_lock(&db).await;

                        component.create_response(ctx, serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new().content("Vote recorded!").ephemeral(true),
                        )).await?;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}