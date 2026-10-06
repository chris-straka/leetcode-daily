# Learning notes: LeetCode Daily Bot

## 1. The problem

Practice sticks better with company. The bot turns LeetCode into a small game
for a Discord server: one shared problem a day, points for solving it, a
penalty for skipping, a monthly winner and reminders before contests. The
interesting engineering is that a chat bot has to judge something that happens
on another site. It can't see your LeetCode account, so it asks LeetCode's
public GraphQL API for your recent accepted submissions and decides from that.

Companies build the same shape all the time: a long-running async service that
reacts to webhook or gateway events, runs scheduled jobs, calls a third-party
API it doesn't control, and keeps a small amount of state that must not be
lost or double-counted.

## 2. Concepts you need

**Gateway bot with slash commands.** Discord pushes events (messages, command
invocations) over a websocket; the bot never polls Discord. `poise` sits on
`serenity` and turns `#[poise::command]` functions into slash commands, which
are registered with Discord at startup
([src/main.rs:50](../src/main.rs#L50)). `MESSAGE_CONTENT` is a privileged
intent, which is why the README tells you to enable it in the portal.

**Pure core, effectful shell.** Every game rule (points, first-solver bonus,
rollover penalty, monthly winners, contest alert timing) lives in
[src/scoring.rs](../src/scoring.rs) and takes plain data. Discord and HTTP calls
stay in `commands.rs`, `events.rs` and `tasks.rs`. That split is what makes 16
of the 26 tests possible without a network.

**Check-and-set under one lock.** Two paths can verify the same solve: pasting a
code block in the thread, and `/claim`. Both call LeetCode first (slow, no lock
held), then take the database write lock and call `award`
([src/scoring.rs:48](../src/scoring.rs#L48)), which re-checks "already credited?"
and updates in the same critical section
([src/events.rs:181](../src/events.rs#L181),
[src/commands.rs:425](../src/commands.rs#L425)). Checking under a read lock and
writing later under a write lock is the classic time-of-check/time-of-use bug,
and was how users could score twice.

**Verification window.** LeetCode reuses old problems as dailies, and the
recent-submissions query returns the last 30 accepts with no date filter. So a
solve only counts if its timestamp is at or after 00:00 UTC on the day the
daily was posted ([src/scoring.rs:78](../src/scoring.rs#L78),
[src/scoring.rs:91](../src/scoring.rs#L91)).

**Atomic file persistence.** All state is one `HashMap<GuildId, GuildData>`
behind a `tokio::sync::RwLock`, saved as `database.json`. Saves write a temp
file and `rename` it over the old one, so a crash mid-write leaves the previous
version ([src/models.rs:76](../src/models.rs#L76)). Loading refuses a file that
exists but doesn't parse, because starting empty would overwrite every server's
scores on the next save ([src/models.rs:59](../src/models.rs#L59)).

**Polling loops that tolerate lateness.** Five `tokio::spawn`ed loops run the
schedule: daily post, NeetCode post, contest alerts, monthly winner and a
catch-up sweep ([src/tasks.rs](../src/tasks.rs)). They poll on a sleep, and a
sleep plus request latency drifts. Contest alerts therefore fire if the poll
lands anywhere in a 15-minute window after each threshold
([src/scoring.rs:169](../src/scoring.rs#L169)), and a per-contest key list stops
repeats. Daily posts compare a stored date string instead of hoping to wake at
midnight.

**Contract tests for an API you don't own.** LeetCode's GraphQL schema is
undocumented. Fixture tests parse trimmed real responses
([src/leetcode.rs:247](../src/leetcode.rs#L247)), so a renamed field fails in CI
instead of silently failing every verification.

## 3. Reading order

1. `src/models.rs`: the whole state model and how it reaches disk. Small, and
   everything else reads or writes these structs.
2. `src/scoring.rs`: the rules, with their tests at the bottom. Read a test,
   then the function it tests.
3. `src/leetcode.rs`: the GraphQL queries and response types.
4. `src/events.rs`: what happens when someone pastes a code block, including the
   in-flight guard (`ProcessingGuard`, [src/events.rs:21](../src/events.rs#L21))
   that drops duplicate concurrent checks.
5. `src/commands.rs`: the slash commands. `/claim` is the longest and repeats
   the verification flow without a message.
6. `src/tasks.rs`: the background loops; note how the old thread is swept by
   `catchup_thread` before it is deleted at rollover.
7. `src/main.rs`: wiring: intents, command list, spawning the loops.

## 4. Exercises

1. **Change the scoring.** Make Hard worth 5 points in `base_points`. Predict
   which tests fail, then run `cargo test scoring`.
   <details><summary>Answer</summary>`points_by_difficulty` and
   `first_solver_gets_a_bonus_and_only_once` (it expects 4 and 3 for Hard).
   The rest use Easy/Medium.</details>

2. **Narrow the alert window.** Set `ALERT_WINDOW_SECS` to `5 * 60` and run
   `cargo test scoring`. Which test breaks? Then change the starting `t` in
   `a_late_poll_cannot_skip_an_alert` from `25 * 3600` to a few other values.
   <details><summary>Answer</summary>`contest_stages_at_their_thresholds`
   fails: 1 s before start is no longer inside the 15-minute stage's window.
   The late-poll test still passes from its original start by luck of phase.
   With a 300 s window and a 307 s step, some starting offsets step right over
   a window, which is exactly the bug the 15-minute window fixed.</details>

3. **Reintroduce the double award.** In `award`, delete the `if solved(user)
   { return None; }` check. Run the tests. Then explain
   why the real fix also needed the callers to hold the write lock.
   <details><summary>Answer</summary>`a_second_verification_of_the_same_solve_scores_nothing`
   fails. Even with the check in place, if callers checked under a read lock
   and released it before writing, two tasks could both pass the check; doing
   both inside one write-lock section makes it atomic.</details>

4. **Corrupt the database.** Write `{"1": {"users":` into a temp
   `database.json` and call `load_db` on it (copy
   `corrupt_file_is_an_error_not_an_empty_db`). What would happen with the old
   `unwrap_or_default()` loader on the next save?
   <details><summary>Answer</summary>It would start with an empty map, and the
   first save would replace every server's data with `{}`.</details>

5. **Probe the live API.** Send the daily query yourself:
   `curl -s -H 'Content-Type: application/json' -X POST https://leetcode.com/graphql -d '{"query":"query { activeDailyCodingChallengeQuestion { link question { titleSlug difficulty } } }"}'`.
   Compare it with the fixture in `parses_the_daily_challenge`.
   <details><summary>Answer</summary>Same field names; values change daily. If
   they ever differ, update the struct and the fixture together.</details>

6. **Add a streak.** Add `streak: u32` to `Status`, bump it in `award` and reset
   it in `daily_rollover` for non-solvers. Old `database.json` files must still
   load. Why do they?
   <details><summary>Answer</summary>`Status` has `#[serde(default)]`, so a
   missing field becomes `0`. A test like
   `old_files_with_active_daily_still_load` should cover it.</details>

7. **Harder: stop losing late solves.** At rollover the old thread is swept
   once, then deleted. A `/claim` made one minute after rollover for
   yesterday's problem gets nothing. Sketch a fix and its test.
   <details><summary>Answer</summary>One option: keep the previous day's slug
   and post date for a grace period, and let `/claim` award against it if the
   accept is inside yesterday's window. The window logic is already pure
   (`window_start`, `solved_since`), so the test needs no network.</details>

## 5. Interview questions

**How do you verify a solve on a site you don't control?** Ask LeetCode's
public GraphQL for the user's last 30 accepted submissions and look for
today's slug with a timestamp after the daily was posted. It needs a public
profile and trusts LeetCode's data; it can't prove the user wrote the code.

**Why a JSON file and not a database?** One process, a few servers, a few dozen
users. A single file with atomic rename is crash-safe and has no ops cost. It
stops scaling when you need several processes, history queries, or write
rates where rewriting the whole file each time hurts; then SQLite is the next
step.

**What was the double-scoring bug?** A check under a read lock, then a network
call, then a write. Two verifications of the same solve both passed the check.
The fix makes the check and the update one function called under the write
lock.

**Why is `std::sync::Mutex` fine for `processing` but the DB uses
`tokio::sync::RwLock`?** The processing set is only touched briefly and never
across an `.await`. The DB lock is held across `save_db().await`, and a std
lock held across an await can block the runtime thread (and isn't `Send`).

**How do the scheduled jobs survive a restart?** They key on stored state
(`last_daily_date`, `last_processed_month`, `alerted_contests`) rather than on
in-memory timers, so a restarted bot sees "today's daily not posted yet" and
posts it.

**What happens if LeetCode is down?** Daily posting skips that minute and tries
again; verification replies with an error and scores nothing; `/claim` finds
nothing. No state changes on failure.

**How would you test the Discord-facing code?** Move more decisions into pure
functions (as `scoring.rs` does), and for the rest put the LeetCode client
behind a trait so tests can inject canned submissions. The Discord calls
themselves are thin.

**What would you change to run this for 10,000 servers?** Shard the gateway
connection (Discord requires it), move state to a real database with per-guild
rows, fetch the daily once and fan out, and rate-limit LeetCode calls with a
shared cache, since every guild asks about the same problem.

## 6. Connections

This sits with the other web/full-stack projects in `~/ResumeProjects`:
`bugtracker` (Express + Postgres + Redis REST API, where auth and pagination
are the focus), `game-store` and `pethreon`. Compared with those, this one is
the long-running async service: background jobs, idempotent state changes and
a third-party API.

For the deeper versions of ideas used here: `durable` is a workflow engine,
i.e. what the hand-rolled polling loops in `tasks.rs` grow into when jobs need
retries and exactly-once steps. `lsmstore` and `sqlstore` show crash-safe
storage done properly (WAL, fsync) where this repo uses write-then-rename.
