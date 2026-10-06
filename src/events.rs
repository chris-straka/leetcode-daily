use crate::models::{Data, Error};
use crate::scoring::{Daily, award};
use poise::serenity_prelude as serenity;
use regex::Regex;
use std::sync::{Arc, LazyLock};

pub static CODE_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)```.+```").unwrap());

// Regex to catch Instagram Reels, Posts, or TV links
static IG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://(?:www\.)?instagram\.com/(?:reels?|p|tv)/[A-Za-z0-9_-]+").unwrap()
});

/// Rewrites the first Instagram post/reel link to vxinstagram, which Discord
/// can embed.
fn embeddable_instagram_link(content: &str) -> Option<String> {
    let url = IG_RE.find(content)?.as_str();
    Some(url.replacen("instagram.com", "vxinstagram.com", 1))
}

struct ProcessingGuard {
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
    thread_type: String,
    processing: Arc<
        std::sync::Mutex<std::collections::HashSet<(serenity::GuildId, serenity::UserId, String)>>,
    >,
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
        let _ = msg
            .reply(
                ctx,
                "❌ Please run `/register <your_leetcode_username>` first!",
            )
            .await;
        return Ok(());
    };

    let (target_slug, difficulty, posted) = if is_lc_thread {
        let (db_slug, db_diff, db_date) = {
            let db = data.db.read().await;
            if let Some(g) = db.get(&guild_id) {
                (
                    g.last_daily_slug.clone(),
                    g.last_daily_diff.clone(),
                    g.last_daily_date.clone(),
                )
            } else {
                (None, None, None)
            }
        };

        if let (Some(s), Some(d)) = (db_slug, db_diff) {
            (s, d, db_date)
        } else {
            let daily = match crate::leetcode::fetch_daily_question().await {
                Ok(d) => d,
                Err(_) => {
                    let _ = msg.reply(ctx, "Error contacting LeetCode API.").await;
                    return Ok(());
                }
            };
            (daily.question.title_slug, daily.question.difficulty, None)
        }
    } else {
        let (db_slug, db_diff, db_date) = {
            let db = data.db.read().await;
            if let Some(g) = db.get(&guild_id) {
                (
                    g.last_neetcode_slug.clone(),
                    g.last_neetcode_diff.clone(),
                    g.last_neetcode_date.clone(),
                )
            } else {
                (None, None, None)
            }
        };

        if let (Some(s), Some(d)) = (db_slug, db_diff) {
            (s, d, db_date)
        } else {
            let slug =
                crate::scoring::neetcode_slug_for(chrono::Utc::now().date_naive()).to_string();

            let diff = match crate::leetcode::fetch_question_by_slug(&slug).await {
                Ok(q) => q.difficulty,
                Err(_) => "Medium".to_string(),
            };
            (slug, diff, None)
        }
    };

    let subs = match crate::leetcode::fetch_recent_ac_submissions(&username).await {
        Ok(s) => s,
        Err(_) => {
            let _ = msg
                .reply(ctx, "Error fetching your profile. Is it public?")
                .await;
            return Ok(());
        }
    };

    let since = crate::scoring::window_start(posted.as_deref(), chrono::Utc::now().date_naive());
    let is_accepted = crate::scoring::solved_since(&subs, &target_slug, since);

    if !is_accepted {
        let _ = msg.reply(ctx, "❌ Couldn't find an Accepted submission! (Wait a few seconds after submitting to LeetCode).").await;
        return Ok(());
    }

    let daily = if is_lc_thread {
        Daily::LeetCode
    } else {
        Daily::NeetCode
    };
    let mut db = data.db.write().await;
    let guild_data = db.entry(guild_id).or_default();
    let Some(won) = award(guild_data, msg.author.id, daily, &difficulty, msg.link()) else {
        return Ok(()); // credited by /claim or another message while we checked
    };
    let main_channel = guild_data.channel_id;
    data.save_from_lock(&db).await;
    drop(db);

    if won.first
        && let Some(main_channel) = main_channel
    {
        let announcement = format!(
            "🥇 **<@{}>** is the first to solve today's {} daily! (+1 bonus pt)",
            msg.author.id,
            daily.name()
        );
        let _ = main_channel.say(&ctx.http, announcement).await;
    }

    let response = format!("✅ Verified via API! +**{}** pts.", won.points);
    let _ = msg.reply(ctx, response).await;

    Ok(())
}

pub async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    if let serenity::FullEvent::Message { new_message: msg } = event {
        if msg.author.bot || msg.guild_id.is_none() {
            return Ok(());
        }

        let guild_id = msg.guild_id.unwrap();

        if let Some(fixed_url) = embeddable_instagram_link(&msg.content) {
            let _ = msg.reply(ctx, fixed_url).await;
        }

        let _ = process_solution_message(ctx, msg, data, guild_id).await;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_blocks_are_detected_across_lines() {
        assert!(CODE_BLOCK_RE.is_match("here:\n```rust\nfn main() {}\n```"));
        assert!(!CODE_BLOCK_RE.is_match("I solved it!"));
        assert!(!CODE_BLOCK_RE.is_match("```unterminated"));
    }

    #[test]
    fn instagram_links_are_rewritten_once() {
        assert_eq!(
            embeddable_instagram_link("look https://www.instagram.com/reel/AbC_1-x/?igsh=1 lol")
                .as_deref(),
            Some("https://www.vxinstagram.com/reel/AbC_1-x")
        );
        assert_eq!(
            embeddable_instagram_link("https://instagram.com/p/xyz").as_deref(),
            Some("https://vxinstagram.com/p/xyz")
        );
        assert_eq!(
            embeddable_instagram_link("https://instagram.com/someuser"),
            None
        );
    }
}
