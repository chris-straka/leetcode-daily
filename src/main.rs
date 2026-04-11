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
    let token = std::env::var("DISCORD_TOKEN").expect("Missing DISCORD_TOKEN");

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::register(),
                commands::scores(),
                commands::random(),
                commands::channel(),
                commands::poll(),
            ],
            event_handler: |ctx, event, framework, data| {
                Box::pin(events::event_handler(ctx, event, framework, data))
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

                let task_data = Arc::new(data.clone());
                let task_ctx = Arc::new(ctx.clone());
                tokio::spawn(async move {
                    tasks::schedule_daily_question(task_ctx, task_data).await;
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
