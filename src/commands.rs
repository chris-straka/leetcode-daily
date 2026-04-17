use crate::models::{Context, Error};
use poise::serenity_prelude as serenity;
use rand::seq::IndexedRandom;

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
        if s.score > 0 { msg.push_str(&format!("{}. <@{}>: **{}** pts\n", p+1, id, s.score)); }
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
        g.active_daily = true;
    }
    ctx.data().save().await;
    ctx.say(format!("✅ Configured to <#{}>", channel.id())).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn register(ctx: Context<'_>, username: String) -> Result<(), Error> {
    ctx.defer().await?;
    let rating = crate::leetcode::fetch_user_rating(&username).await.unwrap_or(0.0);
    let mut db = ctx.data().db.write().await;
    let u = db.entry(ctx.guild_id().unwrap()).or_default().users.entry(ctx.author().id).or_default();
    u.leetcode_username = Some(username.clone());
    u.contest_rating = rating;
    ctx.data().save_from_lock(&db).await;
    ctx.say(format!("✅ Linked **{}** (Rating: {:.0})", username, rating)).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn random(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let qs = crate::leetcode::fetch_all_questions().await?;

    // RNG is scoped inside this block so it is dropped before the .await below
    let picked = {
        let mut rng = rand::rng();
        qs.choose(&mut rng).cloned()
    };

    if let Some(q) = picked {
        let link = format!("/problems/{}", q.title.to_lowercase().replace(' ', "-"));
        ctx.send(poise::CreateReply::default().embed(crate::leetcode::create_embed(&q, &link))).await?;
    } else {
        ctx.say("No questions found.").await?;
    }
    Ok(())
}

#[poise::command(slash_command)]
pub async fn poll(ctx: Context<'_>) -> Result<(), Error> { ctx.defer().await?; ctx.say("Poll started.").await?; Ok(()) }

#[poise::command(slash_command, required_permissions = "MANAGE_GUILD")]
pub async fn contest_setup(ctx: Context<'_>, channel: serenity::Channel) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().unwrap();
    let mut db = ctx.data().db.write().await;
    let g = db.entry(gid).or_default();
    g.weekly_id = Some(channel.id());
    g.active_weekly = true;
    ctx.data().save_from_lock(&db).await;
    ctx.say("✅ Contests set.").await?; Ok(())
}

#[poise::command(slash_command)]
pub async fn ratings(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let gid = ctx.guild_id().ok_or("Must be in guild")?;
    let db = ctx.data().db.read().await;
    let g = db.get(&gid).ok_or("Server not configured.")?;
    
    let mut lb: Vec<_> = g.users.iter().filter(|(_, s)| s.contest_rating > 0.0).collect();
    lb.sort_by(|a, b| b.1.contest_rating.partial_cmp(&a.1.contest_rating).unwrap());
    
    let mut msg = String::from("**🏆 Ratings:**\n");
    for (p, (id, s)) in lb.into_iter().enumerate() {
        msg.push_str(&format!("{}. <@{}>: **{:.0}**\n", p+1, id, s.contest_rating));
    }
    ctx.say(msg).await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn daily(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let challenge = crate::leetcode::fetch_daily_question().await?;
    let embed = crate::leetcode::create_embed(&challenge.question, &challenge.link);
    
    ctx.send(poise::CreateReply::default()
        .content("☀️ **Today's Daily Challenge:**")
        .embed(embed))
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