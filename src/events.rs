use crate::models::{Data, Error};
use poise::serenity_prelude as serenity;
use chrono::{Utc, TimeZone};
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
            if msg.author.bot || msg.guild_id.is_none() { return Ok(()); }

            if CODE_BLOCK_RE.is_match(&msg.content) {
                // 1. Get the username
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

                // 2. Fetch today's daily question slug
                let daily = match crate::leetcode::fetch_daily_question().await {
                    Ok(d) => d,
                    Err(_) => { msg.reply(ctx, "Error contacting LeetCode API.").await?; return Ok(()); }
                };
                // extract "two-sum" from "/problems/two-sum/"
                let daily_slug = daily.link.trim_matches('/').split('/').last().unwrap_or_default();

                // 3. Fetch user's recent Accepted submissions
                let subs = match crate::leetcode::fetch_recent_ac_submissions(&username).await {
                    Ok(s) => s,
                    Err(_) => { msg.reply(ctx, "Error fetching your profile. Is it public?").await?; return Ok(()); }
                };

                // 4. Verify completion
                let is_accepted = subs.iter().any(|sub| sub.title_slug == daily_slug);

                if !is_accepted {
                    msg.reply(ctx, "❌ Couldn't find an Accepted submission for today's daily! (Wait a few seconds after submitting to LeetCode).").await?;
                    return Ok(());
                }

                // 5. Success! Grant points (This is your original logic)
                let mut db = data.db.write().await;
                let guild_data = db.entry(msg.guild_id.unwrap()).or_default();
                
                if guild_data.active_daily && Some(msg.channel_id) == guild_data.thread_id {
                    let user = guild_data.users.entry(msg.author.id).or_default();
                    if user.submitted.is_none() {
                        user.submitted = Some(msg.link());
                        user.monthly_record += 1;
                        
                        let now = Utc::now();
                        let tomorrow = Utc.from_utc_datetime(&now.date_naive().succ_opt().unwrap().and_hms_opt(0, 1, 0).unwrap());
                        let hours_left = (tomorrow - now).num_hours();
                        
                        let score = match hours_left { 23 => 5, 21..=22 => 4, 16..=20 => 3, 8..=15 => 2, _ => 1 };
                        user.score += score;
                        user.days_missed = 0;
                        
                        msg.reply(ctx, format!("✅ Verified via API! +**{}** pts. Month total: **{}**.", score, user.monthly_record)).await?;
                        data.save().await;
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
                            component.create_response(ctx, serenity::CreateInteractionResponse::Message(serenity::CreateInteractionResponseMessage::new().content("Self-voting is cringe.").ephemeral(true))).await?;
                            return Ok(());
                        }
                        let mut db = data.db.write().await;
                        let user = db.entry(component.guild_id.unwrap()).or_default().users.entry(component.user.id).or_default();
                        user.voted_for = Some(voted_for);
                        data.save().await;
                        component.create_response(ctx, serenity::CreateInteractionResponse::Message(serenity::CreateInteractionResponseMessage::new().content("Vote recorded!").ephemeral(true))).await?;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}