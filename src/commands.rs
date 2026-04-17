use crate::models::{Context, Error};
use poise::ChoiceParameter;
use poise::serenity_prelude as serenity;
use rand::seq::SliceRandom;

#[poise::command(slash_command)]
pub async fn scores(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be run in a guild")?;
    let db = ctx.data().db.read().await;
    let guild_data = db
        .get(&guild_id)
        .ok_or("Server not configured. Run /channel first.")?;

    let mut leaderboard: Vec<_> = guild_data.users.iter().collect();
    leaderboard.sort_by(|a, b| {
        b.1.score
            .cmp(&a.1.score)
            .then_with(|| b.1.monthly_record.cmp(&a.1.monthly_record))
    });

    let mut msg = String::from("**The Current Leaderboard:**\n");
    let mut has_score = false;
    for (place, (user_id, status)) in leaderboard.into_iter().enumerate() {
        if status.score > 0 {
            has_score = true;
            msg.push_str(&format!(
                "{}. <@{}> \t**{}** pts \t**{}** completed this month\n",
                place + 1,
                user_id,
                status.score,
                status.monthly_record
            ));
        }
    }
    if !has_score {
        msg.push_str("No one has any points yet.");
    }
    ctx.say(msg).await?;
    Ok(())
}

#[derive(poise::ChoiceParameter)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

#[derive(poise::ChoiceParameter)]
pub enum PaidStatus {
    Free,
    Paid,
}

#[poise::command(slash_command)]
pub async fn random(
    ctx: Context<'_>,
    difficulty: Option<Difficulty>,
    status: Option<PaidStatus>,
) -> Result<(), Error> {
    let qs = crate::leetcode::fetch_all_questions()
        .await
        .map_err(|e| e.to_string())?;

    let filtered: Vec<_> = qs
        .iter()
        .filter(|q| {
            let diff_match = difficulty
                .as_ref()
                .map_or(true, |d| q.difficulty.eq_ignore_ascii_case(&d.name()));
            let paid_match = status.as_ref().map_or(true, |s| match s {
                PaidStatus::Free => !q.is_paid_only,
                PaidStatus::Paid => q.is_paid_only,
            });
            diff_match && paid_match
        })
        .collect();

    let question = {
        let mut rng = rand::thread_rng();
        filtered.choose(&mut rng).map(|&q| q.clone())
    };

    if let Some(q) = question {
        let slug = q
            .title
            .to_lowercase()
            .replace(' ', "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>();
        let link = format!("/problems/{}", slug);
        let embed = crate::leetcode::create_embed(&q, &link);
        ctx.send(
            poise::CreateReply::default()
                .content("Here's a random question!")
                .embed(embed),
        )
        .await?;
    } else {
        ctx.say("No questions found matching your criteria.")
            .await?;
    }
    Ok(())
}

#[poise::command(slash_command, required_permissions = "MANAGE_GUILD")]
pub async fn channel(ctx: Context<'_>, channel: serenity::GuildChannel) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let members = guild_id.members(ctx.http(), None, None).await?;

    let mut db = ctx.data().db.write().await;
    let guild_data = db.entry(guild_id).or_default();
    guild_data.channel_id = Some(channel.id);
    guild_data.active_daily = true;

    for member in members {
        if !member.user.bot {
            guild_data.users.entry(member.user.id).or_default();
        }
    }

    ctx.data().save().await;
    ctx.say(format!(
        "Configured! Broadcasting dailies to <#{}>.",
        channel.id
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn poll(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let mut db = ctx.data().db.write().await;
    let guild_data = db.entry(guild_id).or_default();

    let mut msg = String::from("**Choose your favourite submission:**\n");
    let mut has_subs = false;
    for (id, status) in guild_data.users.iter() {
        if let Some(sub) = &status.submitted {
            has_subs = true;
            msg.push_str(&format!("<@{}>: {}\n", id, sub));
        }
    }

    if !has_subs {
        ctx.say("No submissions to vote on yet!").await?;
        return Ok(());
    }

    let select_menu = serenity::CreateSelectMenu::new(
        "favourite_submission",
        serenity::CreateSelectMenuKind::User {
            default_users: None,
        },
    );
    let components = vec![serenity::CreateActionRow::SelectMenu(select_menu)];
    let thread_id = guild_data.thread_id.unwrap_or_else(|| ctx.channel_id());

    let sent_msg = thread_id
        .send_message(
            ctx.http(),
            serenity::CreateMessage::new()
                .content(msg)
                .components(components),
        )
        .await?;
    sent_msg.pin(ctx.http()).await?;

    guild_data.poll_id = Some(sent_msg.id);
    ctx.data().save().await;
    ctx.say("Poll created!").await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn register(
    ctx: Context<'_>,
    #[description = "Your exact LeetCode username"] username: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be run in a guild")?;

    // Fetch initial rating on register
    let rating = crate::leetcode::fetch_user_rating(&username)
        .await
        .unwrap_or(0.0);

    let mut db = ctx.data().db.write().await;
    let guild_data = db.entry(guild_id).or_default();
    let user = guild_data.users.entry(ctx.author().id).or_default();

    user.leetcode_username = Some(username.clone());
    user.contest_rating = rating;
    ctx.data().save().await;

    ctx.say(format!(
        "✅ Linked LeetCode account: **{}** (Rating: {:.0})",
        username, rating
    ))
    .await?;
    Ok(())
}

// --- NEW CONTEST COMMANDS ---

#[poise::command(slash_command, required_permissions = "MANAGE_GUILD")]
pub async fn contest_setup(ctx: Context<'_>, channel: serenity::GuildChannel) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let mut db = ctx.data().db.write().await;
    let guild_data = db.entry(guild_id).or_default();

    guild_data.weekly_id = Some(channel.id);
    guild_data.active_weekly = true;
    ctx.data().save().await;

    ctx.say(format!(
        "✅ Contest announcements will be sent to <#{}>.",
        channel.id
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn ratings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be run in a guild")?;
    let db = ctx.data().db.read().await;
    let guild_data = db.get(&guild_id).ok_or("Server not configured.")?;

    let mut leaderboard: Vec<_> = guild_data
        .users
        .iter()
        .filter(|(_, s)| s.contest_rating > 0.0)
        .collect();
    leaderboard.sort_by(|a, b| b.1.contest_rating.partial_cmp(&a.1.contest_rating).unwrap());

    let mut msg = String::from("**🏆 Server Contest Ratings:**\n");
    if leaderboard.is_empty() {
        msg.push_str("No registered users have participated in a LeetCode contest.");
    }

    for (place, (user_id, status)) in leaderboard.into_iter().enumerate() {
        let username = status.leetcode_username.as_deref().unwrap_or("Unknown");
        msg.push_str(&format!(
            "{}. <@{}> (`{}`) \t**{:.0}**\n",
            place + 1,
            user_id,
            username,
            status.contest_rating
        ));
    }

    ctx.say(msg).await?;
    Ok(())
}
