mod commands;
mod events;
mod leetcode;
mod models;
mod tasks;

use poise::serenity_prelude as serenity;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let token = std::env::var("DISCORD_TOKEN").expect("Missing DISCORD_TOKEN");

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::register(),
                commands::scores(),
                commands::random(),
                commands::channel(),
                commands::contest_setup(),
                commands::ratings(),
                commands::daily(),
                commands::contests(),
            ],
            event_handler: |ctx, event, framework, data| {
                Box::pin(events::event_handler(ctx, event, framework, data))
            },
            on_error: |error| {
                Box::pin(async move {
                    tracing::error!("Poise error: {:?}", error);
                    if let poise::FrameworkError::Command { error, ctx, .. } = error {
                        let _ = ctx.say(format!("❌ Error: {}", error)).await;
                    }
                })
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                let db_content = tokio::fs::read_to_string("database.json")
                    .await
                    .unwrap_or_default();
                let data = models::Data {
                    db: Arc::new(tokio::sync::RwLock::new(
                        serde_json::from_str(&db_content).unwrap_or_default(),
                    )),
                };

                let t1_data = Arc::new(data.clone());
                let t1_ctx = Arc::new(ctx.clone());
                tokio::spawn(async move {
                    tasks::schedule_daily_question(t1_ctx, t1_data).await;
                });

                let t2_data = Arc::new(data.clone());
                let t2_ctx = Arc::new(ctx.clone());
                tokio::spawn(async move {
                    tasks::schedule_contests(t2_ctx, t2_data).await;
                });

                Ok(data)
            })
        })
        .build();

    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_MEMBERS;

    serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .unwrap()
        .start()
        .await
        .unwrap();
}