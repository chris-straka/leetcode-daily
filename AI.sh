#!/bin/bash

# Use this file to copy and paste files to your clipboard 
# You can then paste it into AI chatbots to get help

# List the files you want to include
FILES=(
  "src/commands.rs"
  "src/events.rs"
  "src/leetcode.rs"
  "src/main.rs"
  "src/models.rs"
  "src/tasks.rs"
  "Cargo.toml"
)

# Clear the output or use a variable
output=""

for file in "${FILES[@]}"; do
  if [ -f "$file" ]; then
    ext="${file##*.}"
    output+="$file\n\n\`\`\`$ext\n$(cat "$file")\n\`\`\`\n\n"
  fi
done

# Copy to clipboard (macOS: pbcopy | Linux: xclip -sel clip)
echo -e "$output" | pbcopy

echo "Project context copied to clipboard!"