#!/bin/bash

SESSION="bot"

# 1. Kill existing screen session and any lingering bot processes
screen -S "$SESSION" -X quit 2>/dev/null
killall leetcode-daily 2>/dev/null

# 2. Build and run the bot
screen -dmS "$SESSION" cargo run

echo "🚀 Bot is compiling and starting in screen: $SESSION"
echo "👉 Use 'screen -r $SESSION' to see logs/output"
echo "👉 Use 'screen -ls to view current bots"