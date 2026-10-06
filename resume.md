# LeetCode Daily Bot — resume bullets

- Built a Discord bot in async Rust (tokio, poise/serenity; ~2.3k lines) that
  runs a daily LeetCode game for a server: it posts the daily problem in a new
  thread, verifies each solve against LeetCode's GraphQL API, and keeps a
  leaderboard with first-solver bonuses, missed-day penalties, monthly winners
  and contest reminders from five background jobs.
- Fixed three scoring bugs: a check-then-act race that let
  one solve score twice (now one check-and-update under the write lock), old
  accepts of a repeated daily counting as today's solve (now limited to the
  post day by submission timestamp), and contest alerts that a late poll could
  skip (a 15-minute window, simulated in a test).
- Made persistence crash-safe: atomic write-then-rename saves, and a loader
  that refuses a corrupt `database.json` instead of starting empty and wiping
  every server's scores on the next save.
- Pulled the game rules out into a pure module and added 26 unit tests,
  including GraphQL fixture tests checked against live LeetCode responses, plus
  a GitHub Actions CI running fmt, clippy `-D warnings`, tests and a release
  build.
