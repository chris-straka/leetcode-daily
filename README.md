# LeetCode Daily Bot

A Discord bot to keep you and your friends motivated doing LeetCode!
Entirely rewritten using `poise` for modern native slash commands.

## Features

- Daily Question fetching with leaderboard tracking.
- Native Discord slash commands (`/scores`, `/poll`, `/channel`, `/random` with dropdown menus).
- Points rewarded automatically when sharing code blocks natively via regex processing.

## Discord Portal Setup

Before inviting the bot, you must enable two **Privileged Gateway Intents** in the Discord Developer Portal under your application's "Bot" tab:

1. **Server Members Intent**: Needed so the bot can auto-enroll your friends when you setup the channel.
2. **Message Content Intent**: Needed so the bot can read your code blocks and award points.
