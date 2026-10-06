//! Pure game rules: points, daily rollover, monthly winners, NeetCode
//! rotation and contest alert timing. Nothing here touches Discord or the
//! network, so all of it is unit-tested below.

use crate::leetcode::Submission;
use crate::models::GuildData;
use chrono::{Datelike, NaiveDate};
use poise::serenity_prelude as serenity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Daily {
    LeetCode,
    NeetCode,
}

impl Daily {
    pub fn name(self) -> &'static str {
        match self {
            Daily::LeetCode => "LeetCode",
            Daily::NeetCode => "NeetCode",
        }
    }
}

/// Points for an accepted solve, before the first-solver bonus.
pub fn base_points(difficulty: &str) -> usize {
    match difficulty {
        "Medium" => 2,
        "Hard" => 3,
        _ => 1,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Award {
    pub points: usize,
    /// First solver in this guild today; already included in `points`.
    pub first: bool,
}

/// Records a verified solve and returns the points granted, or `None` if the
/// user already has today's credit.
///
/// Callers must hold the database write lock across this call. The check and
/// the update happen together, so two concurrent verifications (a code block
/// and `/claim`, say) can't both score, and only one user gets the
/// first-solver bonus.
pub fn award(
    guild: &mut GuildData,
    user_id: serenity::UserId,
    daily: Daily,
    difficulty: &str,
    proof: String,
) -> Option<Award> {
    let solved = |s: &crate::models::Status| match daily {
        Daily::LeetCode => s.submitted.is_some(),
        Daily::NeetCode => s.nc_submitted.is_some(),
    };
    let first = !guild.users.values().any(solved);

    let user = guild.users.entry(user_id).or_default();
    if solved(user) {
        return None;
    }
    let points = base_points(difficulty) + usize::from(first);
    match daily {
        Daily::LeetCode => user.submitted = Some(proof),
        Daily::NeetCode => user.nc_submitted = Some(proof),
    }
    user.monthly_record += 1;
    user.score += points;
    user.days_missed = 0;
    Some(Award { points, first })
}

/// Unix time of 00:00 UTC on the day a daily was posted (`posted` is the
/// stored "%Y-%m-%d"), or on `today` if the post date isn't known.
pub fn window_start(posted: Option<&str>, today: NaiveDate) -> i64 {
    posted
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .unwrap_or(today)
        .and_time(chrono::NaiveTime::MIN)
        .and_utc()
        .timestamp()
}

/// Whether `subs` holds an accepted solve of `slug` made at or after `since`.
/// LeetCode repeats old problems as dailies, so an accept from before the
/// daily was posted doesn't count. A timestamp that doesn't parse is
/// accepted rather than locking everyone out if LeetCode changes its format.
pub fn solved_since(subs: &[Submission], slug: &str, since: i64) -> bool {
    subs.iter()
        .any(|s| s.title_slug == slug && s.timestamp.parse::<i64>().map_or(true, |t| t >= since))
}

/// Starts a new day: everyone who didn't solve the outgoing daily loses a
/// point (never below zero), and every user's solved flag is cleared.
pub fn daily_rollover(guild: &mut GuildData, daily: Daily) {
    for u in guild.users.values_mut() {
        let slot = match daily {
            Daily::LeetCode => &mut u.submitted,
            Daily::NeetCode => &mut u.nc_submitted,
        };
        if slot.take().is_none() {
            u.score = u.score.saturating_sub(1);
            u.days_missed += 1;
        }
    }
}

/// The highest score this month and every user tied on it (sorted, so the
/// announcement order is stable). No winners if nobody scored.
pub fn monthly_winners(guild: &GuildData) -> (usize, Vec<serenity::UserId>) {
    let best = guild.users.values().map(|s| s.score).max().unwrap_or(0);
    if best == 0 {
        return (0, Vec::new());
    }
    let mut ids: Vec<_> = guild
        .users
        .iter()
        .filter(|(_, s)| s.score == best)
        .map(|(id, _)| *id)
        .collect();
    ids.sort();
    (best, ids)
}

/// "December 2025" for a December that ended when `today` is in January 2026.
pub fn finished_month_label(finished_month: u32, today: NaiveDate) -> String {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let year = if finished_month > today.month() {
        today.year() - 1
    } else {
        today.year()
    };
    format!("{} {}", NAMES[(finished_month as usize - 1) % 12], year)
}

/// Today's NeetCode problem: the list is walked one problem per UTC day.
pub fn neetcode_slug_for(date: NaiveDate) -> &'static str {
    let list = crate::neetcode::NEETCODE_250;
    list[date.num_days_from_ce() as usize % list.len()]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContestStage {
    DayBefore,
    HourBefore,
    QuarterHourBefore,
    Started,
}

/// How long each alert stays eligible after its threshold passes. It is far
/// wider than the 5-minute poll interval, so a slow poll can't skip an alert.
/// The per-contest `alerted_contests` keys keep it from repeating.
const ALERT_WINDOW_SECS: i64 = 15 * 60;

/// Which alert, if any, is due `secs_until_start` seconds before a contest.
pub fn contest_stage(secs_until_start: i64) -> Option<ContestStage> {
    let due = |threshold: i64| {
        secs_until_start <= threshold && secs_until_start > threshold - ALERT_WINDOW_SECS
    };
    if due(24 * 3600) {
        Some(ContestStage::DayBefore)
    } else if due(3600) {
        Some(ContestStage::HourBefore)
    } else if due(15 * 60) && secs_until_start > 0 {
        Some(ContestStage::QuarterHourBefore)
    } else if due(0) {
        Some(ContestStage::Started)
    } else {
        None
    }
}

impl ContestStage {
    /// Dedup key stored in `GuildData::alerted_contests`.
    pub fn key(self, title: &str) -> String {
        let suffix = match self {
            ContestStage::DayBefore => "24h",
            ContestStage::HourBefore => "1h",
            ContestStage::QuarterHourBefore => "15m",
            ContestStage::Started => "start",
        };
        format!("{title}-{suffix}")
    }

    pub fn message(self, title: &str) -> String {
        match self {
            ContestStage::DayBefore => {
                format!("📅 **Contest Tomorrow**: {title} starts in 24 hours! Get some sleep.")
            }
            ContestStage::HourBefore => format!("⏰ **1 Hour Warning**: {title} is starting soon!"),
            ContestStage::QuarterHourBefore => {
                format!("🚨 **15 Minutes**: {title} is about to begin. Join the lobby!")
            }
            ContestStage::Started => {
                format!("🚀 **Started**: {title} is live! Good luck everyone!")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Status;

    fn uid(n: u64) -> serenity::UserId {
        serenity::UserId::new(n)
    }

    #[test]
    fn points_by_difficulty() {
        assert_eq!(base_points("Easy"), 1);
        assert_eq!(base_points("Medium"), 2);
        assert_eq!(base_points("Hard"), 3);
        assert_eq!(base_points("???"), 1);
    }

    #[test]
    fn first_solver_gets_a_bonus_and_only_once() {
        let mut g = GuildData::default();
        let a = award(&mut g, uid(1), Daily::LeetCode, "Hard", "p".into());
        let b = award(&mut g, uid(2), Daily::LeetCode, "Hard", "p".into());
        assert_eq!(
            a,
            Some(Award {
                points: 4,
                first: true
            })
        );
        assert_eq!(
            b,
            Some(Award {
                points: 3,
                first: false
            })
        );
    }

    #[test]
    fn a_second_verification_of_the_same_solve_scores_nothing() {
        // The double-award race: a code block and /claim both verify.
        let mut g = GuildData::default();
        assert!(award(&mut g, uid(1), Daily::LeetCode, "Easy", "msg".into()).is_some());
        assert_eq!(
            award(&mut g, uid(1), Daily::LeetCode, "Easy", "claim".into()),
            None
        );
        assert_eq!(g.users[&uid(1)].score, 2);
        assert_eq!(g.users[&uid(1)].monthly_record, 1);
    }

    #[test]
    fn leetcode_and_neetcode_are_scored_independently() {
        let mut g = GuildData::default();
        award(&mut g, uid(1), Daily::LeetCode, "Easy", "p".into());
        let nc = award(&mut g, uid(2), Daily::NeetCode, "Medium", "p".into());
        assert_eq!(
            nc,
            Some(Award {
                points: 3,
                first: true
            })
        );
        assert!(award(&mut g, uid(1), Daily::NeetCode, "Medium", "p".into()).is_some());
    }

    fn sub(slug: &str, timestamp: &str) -> Submission {
        Submission {
            title_slug: slug.into(),
            timestamp: timestamp.into(),
        }
    }

    #[test]
    fn window_starts_at_utc_midnight_of_the_post_day() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        // 2026-10-05T00:00:00Z and 2026-10-06T00:00:00Z
        assert_eq!(window_start(Some("2026-10-05"), today), 1_791_158_400);
        assert_eq!(window_start(None, today), 1_791_244_800);
        assert_eq!(window_start(Some("garbage"), today), 1_791_244_800);
    }

    #[test]
    fn an_old_accept_of_a_repeated_daily_does_not_count() {
        let since = 1_791_244_800;
        let old = [sub("two-sum", "1700000000"), sub("3sum", "1791250000")];
        assert!(!solved_since(&old, "two-sum", since));
        assert!(solved_since(&old, "3sum", since));
        assert!(solved_since(
            &[sub("two-sum", "1791244800")],
            "two-sum",
            since
        ));
        assert!(!solved_since(&old, "4sum", since));
    }

    #[test]
    fn an_unparseable_timestamp_is_not_held_against_the_player() {
        assert!(solved_since(&[sub("two-sum", "")], "two-sum", i64::MAX));
    }

    #[test]
    fn rollover_penalises_non_solvers_and_clears_flags() {
        let mut g = GuildData::default();
        g.users.insert(
            uid(1),
            Status {
                score: 5,
                ..Default::default()
            },
        );
        g.users.insert(
            uid(2),
            Status {
                score: 0,
                ..Default::default()
            },
        );
        award(&mut g, uid(3), Daily::LeetCode, "Easy", "p".into());

        daily_rollover(&mut g, Daily::LeetCode);

        assert_eq!(g.users[&uid(1)].score, 4);
        assert_eq!(g.users[&uid(2)].score, 0, "never below zero");
        assert_eq!(g.users[&uid(3)].score, 2, "solver keeps points");
        assert_eq!(g.users[&uid(1)].days_missed, 1);
        assert_eq!(g.users[&uid(3)].days_missed, 0);
        assert!(g.users.values().all(|u| u.submitted.is_none()));
    }

    #[test]
    fn rollover_only_touches_its_own_daily() {
        let mut g = GuildData::default();
        award(&mut g, uid(1), Daily::NeetCode, "Easy", "p".into());
        daily_rollover(&mut g, Daily::LeetCode);
        assert!(g.users[&uid(1)].nc_submitted.is_some());
    }

    #[test]
    fn monthly_winner_ties_are_all_reported() {
        let mut g = GuildData::default();
        for (id, score) in [(3, 7), (1, 7), (2, 4)] {
            g.users.insert(
                uid(id),
                Status {
                    score,
                    ..Default::default()
                },
            );
        }
        assert_eq!(monthly_winners(&g), (7, vec![uid(1), uid(3)]));
    }

    #[test]
    fn no_monthly_winner_when_nobody_scored() {
        let mut g = GuildData::default();
        g.users.insert(uid(1), Status::default());
        assert_eq!(monthly_winners(&g), (0, vec![]));
    }

    #[test]
    fn month_label_rolls_back_the_year_in_january() {
        let jan = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(finished_month_label(12, jan), "December 2025");
        let jun = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        assert_eq!(finished_month_label(5, jun), "May 2026");
    }

    #[test]
    fn neetcode_rotation_walks_the_list_one_problem_a_day() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        let list = crate::neetcode::NEETCODE_250;
        let i = list
            .iter()
            .position(|s| *s == neetcode_slug_for(d))
            .unwrap();
        let next = neetcode_slug_for(d.succ_opt().unwrap());
        assert_eq!(next, list[(i + 1) % list.len()]);
        let cycle = d + chrono::Days::new(list.len() as u64);
        assert_eq!(neetcode_slug_for(cycle), neetcode_slug_for(d));
    }

    #[test]
    fn contest_stages_at_their_thresholds() {
        assert_eq!(contest_stage(24 * 3600), Some(ContestStage::DayBefore));
        assert_eq!(contest_stage(3600), Some(ContestStage::HourBefore));
        assert_eq!(contest_stage(900), Some(ContestStage::QuarterHourBefore));
        assert_eq!(contest_stage(1), Some(ContestStage::QuarterHourBefore));
        assert_eq!(contest_stage(0), Some(ContestStage::Started));
        assert_eq!(contest_stage(-60), Some(ContestStage::Started));
        assert_eq!(contest_stage(2 * 3600), None);
        assert_eq!(contest_stage(-3600), None);
    }

    #[test]
    fn a_late_poll_cannot_skip_an_alert() {
        // The old windows were 300 s wide and the poll ran every 300 s plus
        // request latency, so a poll could step right over one. Simulate polls
        // that each take 300 s + 7 s and check that every stage still fires.
        let mut seen = Vec::new();
        let mut t = 25 * 3600;
        while t > -1800 {
            if let Some(s) = contest_stage(t)
                && !seen.contains(&s)
            {
                seen.push(s);
            }
            t -= 307;
        }
        assert_eq!(
            seen,
            vec![
                ContestStage::DayBefore,
                ContestStage::HourBefore,
                ContestStage::QuarterHourBefore,
                ContestStage::Started
            ]
        );
    }

    #[test]
    fn alert_keys_match_the_stored_format() {
        assert_eq!(ContestStage::DayBefore.key("Weekly 400"), "Weekly 400-24h");
        assert_eq!(ContestStage::Started.key("Weekly 400"), "Weekly 400-start");
    }
}
