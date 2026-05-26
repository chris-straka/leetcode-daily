use crate::models::{Data, Error};
use poise::serenity_prelude as serenity;
use regex::Regex;
use std::sync::{Arc, LazyLock};

pub static CODE_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)```.+```").unwrap());

// Regex to catch Instagram Reels, Posts, or TV links
static IG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://(?:www\.)?instagram\.com/(?:reels?|p|tv)/[A-Za-z0-9_-]+").unwrap()
});

struct ProcessingGuard {
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
    thread_type: String,
    processing: Arc<std::sync::Mutex<std::collections::HashSet<(serenity::GuildId, serenity::UserId, String)>>>,
}

impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        if let Ok(mut processing) = self.processing.lock() {
            processing.remove(&(self.guild_id, self.user_id, self.thread_type.clone()));
        }
    }
}

pub async fn process_solution_message(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    if !CODE_BLOCK_RE.is_match(&msg.content) {
        return Ok(());
    }

    let (is_lc_thread, is_nc_thread, username_opt, already_submitted) = {
        let db = data.db.read().await;
        if let Some(g) = db.get(&guild_id) {
            let lc = g.active_leetcode && Some(msg.channel_id) == g.thread_id;
            let nc = g.active_neetcode && Some(msg.channel_id) == g.neetcode_thread_id;
            let user = g.users.get(&msg.author.id);
            let uname = user.and_then(|u| u.leetcode_username.clone());
            let submitted = if lc {
                user.is_some_and(|u| u.submitted.is_some())
            } else {
                user.is_some_and(|u| u.nc_submitted.is_some())
            };
            (lc, nc, uname, submitted)
        } else {
            (false, false, None, false)
        }
    };

    if !is_lc_thread && !is_nc_thread {
        return Ok(());
    }

    if already_submitted {
        return Ok(());
    }

    let thread_type = if is_lc_thread { "leetcode" } else { "neetcode" };

    // Prevent concurrent checks
    {
        let mut processing = data.processing.lock().unwrap();
        let key = (guild_id, msg.author.id, thread_type.to_string());
        if processing.contains(&key) {
            return Ok(());
        }
        processing.insert(key);
    }

    // Auto-remove processing lock when function completes or errors
    let _guard = ProcessingGuard {
        guild_id,
        user_id: msg.author.id,
        thread_type: thread_type.to_string(),
        processing: data.processing.clone(),
    };

    let Some(username) = username_opt else {
        let _ = msg.reply(
            ctx,
            "❌ Please run `/register <your_leetcode_username>` first!",
        ).await;
        return Ok(());
    };

    let (target_slug, difficulty) = if is_lc_thread {
        let (db_slug, db_diff) = {
            let db = data.db.read().await;
            if let Some(g) = db.get(&guild_id) {
                (g.last_daily_slug.clone(), g.last_daily_diff.clone())
            } else {
                (None, None)
            }
        };
        
        if let (Some(s), Some(d)) = (db_slug, db_diff) {
            (s, d)
        } else {
            let daily = match crate::leetcode::fetch_daily_question().await {
                Ok(d) => d,
                Err(_) => {
                    let _ = msg.reply(ctx, "Error contacting LeetCode API.").await;
                    return Ok(());
                }
            };
            (daily.question.title_slug, daily.question.difficulty)
        }
    } else {
        let (db_slug, db_diff) = {
            let db = data.db.read().await;
            if let Some(g) = db.get(&guild_id) {
                (g.last_neetcode_slug.clone(), g.last_neetcode_diff.clone())
            } else {
                (None, None)
            }
        };
        
        if let (Some(s), Some(d)) = (db_slug, db_diff) {
            (s, d)
        } else {
            use chrono::Datelike;
            let days = chrono::Utc::now().num_days_from_ce();
            let index = (days as usize) % crate::neetcode::NEETCODE_250.len();
            let slug = crate::neetcode::NEETCODE_250[index].to_string();

            let diff = match crate::leetcode::fetch_question_by_slug(&slug).await {
                Ok(q) => q.difficulty,
                Err(_) => "Medium".to_string(),
            };
            (slug, diff)
        }
    };

    let subs = match crate::leetcode::fetch_recent_ac_submissions(&username).await {
        Ok(s) => s,
        Err(_) => {
            let _ = msg.reply(ctx, "Error fetching your profile. Is it public?").await;
            return Ok(());
        }
    };

    let is_accepted = subs.iter().any(|sub| sub.title_slug == target_slug);

    if !is_accepted {
        let _ = msg.reply(ctx, "❌ Couldn't find an Accepted submission! (Wait a few seconds after submitting to LeetCode).").await;
        return Ok(());
    }

    let mut db = data.db.write().await;
    let guild_data = db.entry(guild_id).or_default();

    let solvers_so_far = guild_data
        .users
        .values()
        .filter(|u| {
            if is_lc_thread {
                u.submitted.is_some()
            } else {
                u.nc_submitted.is_some()
            }
        })
        .count();

    let user = guild_data.users.entry(msg.author.id).or_default();
    
    let already_submitted_now = if is_lc_thread {
        user.submitted.is_some()
    } else {
        user.nc_submitted.is_some()
    };

    if !already_submitted_now {
        let base_score = match difficulty.as_str() {
            "Easy" => 1,
            "Medium" => 2,
            "Hard" => 3,
            _ => 1,
        };

        let mut total_gain = base_score;
        if solvers_so_far == 0 {
            total_gain += 1;
        }

        if is_lc_thread {
            user.submitted = Some(msg.link());
        } else {
            user.nc_submitted = Some(msg.link());
        }
        user.monthly_record += 1;
        user.score += total_gain;
        user.days_missed = 0;

        if solvers_so_far == 0 {
            if let Some(main_channel) = guild_data.channel_id {
                let announcement = format!(
                    "🥇 **<@{}>** is the first to solve today's {} daily! (+1 bonus pt)",
                    msg.author.id,
                    if is_lc_thread { "LeetCode" } else { "NeetCode" }
                );
                let _ = main_channel.say(&ctx.http, announcement).await;
            }
        }

        let response = format!("✅ Verified via API! +**{}** pts.", total_gain);
        let _ = msg.reply(ctx, response).await;
        data.save_from_lock(&db).await;
    }

    Ok(())
}

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

            let guild_id = msg.guild_id.unwrap();

            if let Some(caps) = IG_RE.captures(&msg.content) {
                let original_url = caps.get(0).unwrap().as_str();
                let fixed_url = original_url.replace("instagram.com", "vxinstagram.com");
                let _ = msg.reply(ctx, format!("{}", fixed_url)).await;
            }

            let _ = process_solution_message(ctx, msg, data, guild_id).await;
        }
        _ => {}
    }
    Ok(())
}