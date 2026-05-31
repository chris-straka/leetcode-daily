use std::sync::Arc;

use crate::models::{Context, Error};
use chrono::Datelike;
use poise::serenity_prelude as serenity;
use rand::seq::IndexedRandom;

#[poise::command(slash_command)]
pub async fn winners(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().ok_or("Must be in a server.")?;
    let db = ctx.data().db.read().await;
    let g = db.get(&gid).ok_or("Run /channel first")?;

    if g.monthly_winners.is_empty() {
        ctx.say("No Leetcoder of the Month winners yet!").await?;
        return Ok(());
    }

    let mut msg = String::from("**🏆 Leetcoder of the Month Winners:**\n");
    let skip = g.monthly_winners.len().saturating_sub(20);
    for winner in g.monthly_winners.iter().skip(skip) {
        if !winner.user_ids.is_empty() {
            let users_str = winner
                .user_ids
                .iter()
                .map(|id| format!("<@{}>", id))
                .collect::<Vec<_>>()
                .join(", ");
            msg.push_str(&format!(
                "• **{}**: {} ({} pts)\n",
                winner.month_year, users_str, winner.score
            ));
        } else {
            msg.push_str(&format!("• **{}**: No winner\n", winner.month_year));
        }
    }

    ctx.say(msg).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn scores(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().ok_or("Must be in a server.")?;
    let db = ctx.data().db.read().await;
    let g = db.get(&gid).ok_or("Run /channel first")?;

    let mut lb: Vec<_> = g.users.iter().collect();
    lb.sort_by(|a, b| b.1.score.cmp(&a.1.score));

    let mut msg = String::from("**Leaderboard:**\n");
    for (p, (id, s)) in lb.into_iter().enumerate() {
        if s.score > 0 {
            msg.push_str(&format!("{}. <@{}>: **{}** pts\n", p + 1, id, s.score));
        }
    }
    ctx.say(msg).await?;
    Ok(())
}

#[poise::command(slash_command, required_permissions = "MANAGE_GUILD")]
pub async fn channel(ctx: Context<'_>, channel: serenity::Channel) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().ok_or("Must be in a server.")?;
    {
        let mut db = ctx.data().db.write().await;
        let g = db.entry(gid).or_default();
        g.channel_id = Some(channel.id());
        g.active_leetcode = true;
    }
    ctx.data().save().await;
    ctx.say(format!(
        "✅ Configured to <#{}>. Leetcode daily enabled. Use `/toggle` to enable NeetCode 250.",
        channel.id()
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command, required_permissions = "MANAGE_GUILD")]
pub async fn toggle(
    ctx: Context<'_>,
    #[description = "Toggle LeetCode Daily"] leetcode: Option<bool>,
    #[description = "Toggle NeetCode 250 Daily"] neetcode: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().ok_or("Must be in a server.")?;

    let (lc_status, nc_status) = {
        let mut db = ctx.data().db.write().await;
        let g = db.entry(gid).or_default();
        if let Some(lc) = leetcode {
            g.active_leetcode = lc;
        }
        if let Some(nc) = neetcode {
            g.active_neetcode = nc;
        }
        let lc_status = g.active_leetcode;
        let nc_status = g.active_neetcode;
        ctx.data().save_from_lock(&db).await;
        (lc_status, nc_status)
    };

    ctx.say(format!(
        "✅ Settings updated:\n- LeetCode Daily: **{}**\n- NeetCode 250 Daily: **{}**",
        lc_status, nc_status
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn register(ctx: Context<'_>, username: String) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().ok_or("Must be in a server.")?;
    let rating = crate::leetcode::fetch_user_rating(&username)
        .await
        .unwrap_or(0.0);
    
    let mut db = ctx.data().db.write().await;
    let u = db
        .entry(gid)
        .or_default()
        .users
        .entry(ctx.author().id)
        .or_default();
    u.leetcode_username = Some(username.clone());
    u.contest_rating = rating;
    ctx.data().save_from_lock(&db).await;
    
    ctx.say(format!(
        "✅ Linked **{}** (Rating: {:.0})",
        username, rating
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn random(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let qs = crate::leetcode::fetch_all_questions().await?;

    let picked = {
        let mut rng = rand::rng();
        qs.choose(&mut rng).cloned()
    };

    if let Some(q) = picked {
        let link = format!("/problems/{}", q.title_slug);
        ctx.send(poise::CreateReply::default().embed(crate::leetcode::create_embed(&q, &link)))
            .await?;
    } else {
        ctx.say("No questions found.").await?;
    }
    Ok(())
}

#[poise::command(slash_command, required_permissions = "MANAGE_GUILD")]
pub async fn contest_setup(ctx: Context<'_>, channel: serenity::Channel) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().ok_or("Must be in a server.")?;
    let mut db = ctx.data().db.write().await;
    let g = db.entry(gid).or_default();
    g.weekly_id = Some(channel.id());
    g.active_weekly = true;
    ctx.data().save_from_lock(&db).await;
    ctx.say("✅ Contests set.").await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn ratings(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().ok_or("Must be in a server.")?;

    let mut users_to_update = Vec::new();
    {
        let db = ctx.data().db.read().await;
        let g = db.get(&gid).ok_or("Server not configured.")?;
        for (id, u) in &g.users {
            if let Some(username) = &u.leetcode_username {
                users_to_update.push((*id, username.clone()));
            }
        }
    }

    let mut updated_ratings = Vec::new();
    for (id, username) in users_to_update {
        let rating = crate::leetcode::fetch_user_rating(&username)
            .await
            .unwrap_or(0.0);
        updated_ratings.push((id, rating));
    }

    {
        let mut db = ctx.data().db.write().await;
        if let Some(g) = db.get_mut(&gid) {
            for (id, rating) in &updated_ratings {
                if let Some(u) = g.users.get_mut(id) {
                    u.contest_rating = *rating;
                }
            }
        }
        ctx.data().save_from_lock(&db).await;
    }

    updated_ratings.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut msg = String::from("**🏆 Ratings:**\n");
    for (p, (id, rating)) in updated_ratings.into_iter().enumerate() {
        if rating > 0.0 {
            msg.push_str(&format!("{}. <@{}>: **{:.0}**\n", p + 1, id, rating));
        } else {
            msg.push_str(&format!("{}. <@{}>: **Unrated**\n", p + 1, id));
        }
    }
    ctx.say(msg).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn daily(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let challenge = crate::leetcode::fetch_daily_question().await?;
    let embed = crate::leetcode::create_embed(&challenge.question, &challenge.link);

    let mut content = String::from("☀️ **Today's LeetCode Daily:**");

    if let Some(gid) = ctx.guild_id() {
        let db = ctx.data().db.read().await;
        if let Some(g) = db.get(&gid) {
            if let Some(tid) = g.thread_id {
                content.push_str(&format!("\n📝 **Discuss here:** <#{}>", tid));
            }
        }
    }

    ctx.send(poise::CreateReply::default().content(content).embed(embed))
        .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn neetcode(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let days = chrono::Utc::now().num_days_from_ce();
    let slug = crate::neetcode::NEETCODE_250[(days as usize) % crate::neetcode::NEETCODE_250.len()];

    // Fetch only the specific question data
    let question = crate::leetcode::fetch_question_by_slug(slug).await?;
    let link = format!("/problems/{}/", slug);

    let embed = crate::leetcode::create_embed(&question, &link);
    let mut content = String::from("🎯 **Today's NeetCode 250 Daily:**");

    if let Some(gid) = ctx.guild_id() {
        let db = ctx.data().db.read().await;
        if let Some(g) = db.get(&gid) {
            if let Some(tid) = g.neetcode_thread_id {
                content.push_str(&format!("\n📝 **Discuss here:** <#{}>", tid));
            }
        }
    }

    ctx.send(poise::CreateReply::default().content(content).embed(embed))
        .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn contests(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let contests = crate::leetcode::fetch_upcoming_contests().await?;

    let mut msg = String::from("📅 **Upcoming LeetCode Contests:**\n");
    for c in contests {
        msg.push_str(&format!("• **{}**: <t:{}:R>\n", c.title, c.start_time));
    }

    ctx.say(msg).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn claim(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().ok_or("Must be in a server.")?;
    let user_id = ctx.author().id;

    // Prevent concurrent claim checks for the same user
    let key = (gid, user_id, "claim".to_string());
    
    let is_processing = {
        let mut p = ctx.data().processing.lock().unwrap();
        if p.contains(&key) {
            true
        } else {
            p.insert(key.clone());
            false
        }
    }; // Lock is dropped here

    if is_processing {
        ctx.say("⏳ Already checking your submissions...").await?;
        return Ok(());
    }

    struct ClaimGuard {
        key: (serenity::GuildId, serenity::UserId, String),
        processing: Arc<std::sync::Mutex<std::collections::HashSet<(serenity::GuildId, serenity::UserId, String)>>>,
    }
    
    impl Drop for ClaimGuard {
        fn drop(&mut self) {
            if let Ok(mut p) = self.processing.lock() {
                p.remove(&self.key);
            }
        }
    }

    let _guard = ClaimGuard {
        key: key.clone(),
        processing: ctx.data().processing.clone(),
    };

    // 1. Gather required state from DB
    let (
        lc_active, nc_active, 
        mut lc_target, mut lc_diff, 
        mut nc_target, mut nc_diff, 
        username, user_submitted_lc, user_submitted_nc, 
        lc_solvers, nc_solvers, channel_id
    ) = {
        let db = ctx.data().db.read().await;
        let g = db.get(&gid).ok_or("Server not configured.")?;
        
        let u = g.users.get(&user_id);
        let username = u.and_then(|u| u.leetcode_username.clone());
        let user_submitted_lc = u.is_some_and(|u| u.submitted.is_some());
        let user_submitted_nc = u.is_some_and(|u| u.nc_submitted.is_some());
        
        let lc_solvers = g.users.values().filter(|u| u.submitted.is_some()).count();
        let nc_solvers = g.users.values().filter(|u| u.nc_submitted.is_some()).count();
        
        (
            g.active_leetcode, g.active_neetcode,
            g.last_daily_slug.clone(), g.last_daily_diff.clone(),
            g.last_neetcode_slug.clone(), g.last_neetcode_diff.clone(),
            username, user_submitted_lc, user_submitted_nc,
            lc_solvers, nc_solvers, g.channel_id
        )
    };

    let Some(uname) = username else {
        ctx.say("❌ Please run `/register <your_leetcode_username>` first!").await?;
        return Ok(());
    };

    if !lc_active && !nc_active {
        ctx.say("❌ No dailies are active on this server.").await?;
        return Ok(());
    }

    if (user_submitted_lc || !lc_active) && (user_submitted_nc || !nc_active) {
        ctx.say("✅ You have already claimed all active dailies for today!").await?;
        return Ok(());
    }

    // 2. Fallback to fetch targets if missing due to a restart
    if lc_active && lc_target.is_none() {
        if let Ok(daily) = crate::leetcode::fetch_daily_question().await {
            lc_target = Some(daily.question.title_slug);
            lc_diff = Some(daily.question.difficulty);
        }
    }

    if nc_active && nc_target.is_none() {
        use chrono::Datelike;
        let days = chrono::Utc::now().num_days_from_ce();
        let slug = crate::neetcode::NEETCODE_250[(days as usize) % crate::neetcode::NEETCODE_250.len()].to_string();
        nc_target = Some(slug.clone());
        if let Ok(q) = crate::leetcode::fetch_question_by_slug(&slug).await {
            nc_diff = Some(q.difficulty);
        } else {
            nc_diff = Some("Medium".to_string());
        }
    }

    // 3. Fetch submissions and verify
    let subs = crate::leetcode::fetch_recent_ac_submissions(&uname).await.unwrap_or_default();
    
    let mut claimed_lc = false;
    let mut claimed_nc = false;
    let mut total_gain = 0;
    let mut announcements = Vec::new();

    if lc_active && !user_submitted_lc {
        if let Some(ref slug) = lc_target {
            if subs.iter().any(|sub| sub.title_slug == *slug) {
                claimed_lc = true;
                let mut gain = match lc_diff.as_deref().unwrap_or("Medium") {
                    "Easy" => 1, "Medium" => 2, "Hard" => 3, _ => 1,
                };
                if lc_solvers == 0 {
                    gain += 1;
                    announcements.push(format!("🥇 **<@{}>** is the first to solve today's LeetCode daily! (+1 bonus pt)", user_id));
                }
                total_gain += gain;
            }
        }
    }

    if nc_active && !user_submitted_nc {
        if let Some(ref slug) = nc_target {
            if subs.iter().any(|sub| sub.title_slug == *slug) {
                claimed_nc = true;
                let mut gain = match nc_diff.as_deref().unwrap_or("Medium") {
                    "Easy" => 1, "Medium" => 2, "Hard" => 3, _ => 1,
                };
                if nc_solvers == 0 {
                    gain += 1;
                    announcements.push(format!("🥇 **<@{}>** is the first to solve today's NeetCode daily! (+1 bonus pt)", user_id));
                }
                total_gain += gain;
            }
        }
    }

    if !claimed_lc && !claimed_nc {
        ctx.say("❌ Couldn't find a new Accepted submission for today's dailies. (Wait a few seconds after submitting to LeetCode).").await?;
        return Ok(());
    }

    // 4. Save to Database
    {
        let mut db = ctx.data().db.write().await;
        let guild_data = db.entry(gid).or_default();
        let user = guild_data.users.entry(user_id).or_default();
        
        if claimed_lc {
            user.submitted = Some("Claimed via /claim".to_string());
            user.monthly_record += 1;
            user.days_missed = 0;
        }
        if claimed_nc {
            user.nc_submitted = Some("Claimed via /claim".to_string());
            user.monthly_record += 1;
            user.days_missed = 0;
        }
        user.score += total_gain;
        ctx.data().save_from_lock(&db).await;
    }

    // 5. Send Announcements & Response
    if let Some(cid) = channel_id {
        for ann in announcements {
            let _ = cid.say(&ctx.http(), ann).await;
        }
    }

    let mut resp = format!("✅ Verified via API! +**{}** pts.", total_gain);
    match (claimed_lc, claimed_nc) {
        (true, true) => resp.push_str(" (LeetCode & NeetCode)"),
        (true, false) => resp.push_str(" (LeetCode)"),
        (false, true) => resp.push_str(" (NeetCode)"),
        _ => {}
    }
    
    ctx.say(resp).await?;
    Ok(())
}