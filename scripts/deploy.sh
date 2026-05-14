#!/bin/bash

# Name of the screen session
SESSION="bot"

# 1. Kill the existing screen session if it exists
screen -S "$SESSION" -X quit 2>/dev/null

# 2. Start a new detached screen session and run the bot
# -d -m starts it in detached mode immediately
screen -dmS "$SESSION" cargo run

echo "🚀 Bot is compiling and starting in screen: $SESSION"
echo "👉 Use 'screen -r $SESSION' to see logs/output"