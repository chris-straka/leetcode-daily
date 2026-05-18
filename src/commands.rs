use crate::models::{Context, Error};
use chrono::Datelike;
use poise::serenity_prelude as serenity;
use rand::seq::IndexedRandom;

#[poise::command(slash_command)]
pub async fn winners(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().ok_or("Must be in guild")?;
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
    let gid = ctx.guild_id().ok_or("Must be in guild")?;
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
    let gid = ctx.guild_id().unwrap();
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
    let gid = ctx.guild_id().unwrap();

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
    let rating = crate::leetcode::fetch_user_rating(&username)
        .await
        .unwrap_or(0.0);
    let mut db = ctx.data().db.write().await;
    let u = db
        .entry(ctx.guild_id().unwrap())
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
    let gid = ctx.guild_id().unwrap();
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
    let gid = ctx.guild_id().ok_or("Must be in guild")?;

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
