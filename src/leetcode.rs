use cached::proc_macro::cached;
use poise::serenity_prelude as serenity;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

const URL: &str = "https://leetcode.com";

#[derive(Serialize)]
struct GqlQuery {
    query: String,
    variables: serde_json::Value,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub ac_rate: Option<f64>,
    pub difficulty: String,
    #[serde(rename = "questionFrontendId")]
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub title_slug: String,
}

#[derive(Deserialize)]
struct DailyResponse {
    data: DailyData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DailyData {
    active_daily_coding_challenge_question: DailyChallenge,
}

#[derive(Deserialize)]
pub struct DailyChallenge {
    pub link: String,
    pub question: Question,
}

pub async fn fetch_daily_question() -> Result<DailyChallenge, reqwest::Error> {
    let query = r#"query { activeDailyCodingChallengeQuestion { link question { acRate difficulty questionFrontendId isPaidOnly title titleSlug } } }"#;
    let res: DailyResponse = Client::new()
        .post(format!("{URL}/graphql"))
        .json(&GqlQuery {
            query: query.into(),
            variables: json!({}),
        })
        .send()
        .await?
        .json()
        .await?;

    Ok(res.data.active_daily_coding_challenge_question)
}

#[cached(time = 2500000, result = true)]
pub async fn fetch_all_questions() -> Result<Vec<Question>, String> {
    let query = r#"query problemsetQuestionList($categorySlug: String, $limit: Int, $skip: Int, $filters: QuestionListFilterInput) {
  problemsetQuestionList: questionList(
    categorySlug: $categorySlug
    limit: $limit
    skip: $skip
    filters: $filters
  ) {
    questions: data {
      acRate
      difficulty
      questionFrontendId
      title
      titleSlug
    }
  }
}"#;
    let res = Client::new()
        .post(format!("{URL}/graphql"))
        .json(&GqlQuery {
            query: query.into(),
            variables: json!({
                "categorySlug": "",
                "skip": 0,
                "limit": 5000,
                "filters": {}
            }),
        })
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;

    if let Some(errors) = json.get("errors") {
        return Err(format!("GraphQL Errors: {}", errors));
    }

    let questions_val = json
        .pointer("/data/problemsetQuestionList/questions")
        .cloned()
        .ok_or_else(|| "Failed to parse questions from response".to_string())?;

    serde_json::from_value(questions_val).map_err(|e| e.to_string())
}

pub async fn fetch_question_by_slug(slug: &str) -> Result<Question, String> {
    let query = r#"query questionData($titleSlug: String!) {
        question(titleSlug: $titleSlug) {
            questionFrontendId
            title
            titleSlug
            difficulty
            acRate
        }
    }"#;

    let res = Client::new()
        .post(format!("{URL}/graphql"))
        .json(&GqlQuery {
            query: query.into(),
            variables: json!({ "titleSlug": slug }),
        })
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;

    let q_val = json
        .get("data")
        .and_then(|d| d.get("question"))
        .ok_or_else(|| "Question not found".to_string())?;

    serde_json::from_value(q_val.clone()).map_err(|e| e.to_string())
}

pub fn create_embed(question: &Question, link: &str) -> serenity::CreateEmbed {
    let color = match question.difficulty.as_str() {
        "Easy" => serenity::Color::DARK_GREEN,
        "Medium" => serenity::Color::ORANGE,
        "Hard" => serenity::Color::DARK_RED,
        _ => serenity::Color::default(),
    };

    serenity::CreateEmbed::default()
        .title(format!("{}. {}", question.id, question.title.trim()))
        .url(format!("{}{}", URL, link))
        .color(color)
        .field("Difficulty", &question.difficulty, true)
        .field(
            "Acceptance Rate",
            format!("{:.2}%", question.ac_rate.unwrap_or_default()),
            true,
        )
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    pub title_slug: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmissionsResponse {
    data: SubmissionsData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmissionsData {
    recent_ac_submission_list: Vec<Submission>,
}

pub async fn fetch_recent_ac_submissions(
    username: &str,
) -> Result<Vec<Submission>, reqwest::Error> {
    let query = r#"query recentAcSubmissions($username: String!, $limit: Int!) { recentAcSubmissionList(username: $username, limit: $limit) { titleSlug timestamp } }"#;
    let res: SubmissionsResponse = Client::new()
        .post(format!("{}/graphql", URL))
        .json(&GqlQuery {
            query: query.into(),
            variables: json!({ "username": username, "limit": 30 }),
        })
        .send()
        .await?
        .json()
        .await?;
    Ok(res.data.recent_ac_submission_list)
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Contest {
    pub title: String,
    pub start_time: i64,
}

pub async fn fetch_upcoming_contests() -> Result<Vec<Contest>, reqwest::Error> {
    let query = r#"query { topTwoContests { title startTime } }"#;
    let res: serde_json::Value = Client::new()
        .post(format!("{URL}/graphql"))
        .json(&GqlQuery {
            query: query.into(),
            variables: json!({}),
        })
        .send()
        .await?
        .json()
        .await?;

    let contests =
        serde_json::from_value(res["data"]["topTwoContests"].clone()).unwrap_or_default();
    Ok(contests)
}

pub async fn fetch_user_rating(username: &str) -> Result<f64, reqwest::Error> {
    let query = r#"query userContestRankingInfo($username: String!) { userContestRanking(username: $username) { rating } }"#;
    let res: serde_json::Value = Client::new()
        .post(format!("{URL}/graphql"))
        .json(&GqlQuery {
            query: query.into(),
            variables: json!({ "username": username }),
        })
        .send()
        .await?
        .json()
        .await?;

    Ok(res["data"]["userContestRanking"]["rating"]
        .as_f64()
        .unwrap_or(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed copies of real leetcode.com/graphql responses. If LeetCode renames
    // a field these fail here instead of silently breaking verification.

    #[test]
    fn parses_the_daily_challenge() {
        let json = r#"{"data":{"activeDailyCodingChallengeQuestion":{
            "link":"/problems/two-sum/",
            "question":{"acRate":55.1,"difficulty":"Easy","questionFrontendId":"1",
                        "isPaidOnly":false,"title":"Two Sum","titleSlug":"two-sum"}}}}"#;
        let res: DailyResponse = serde_json::from_str(json).unwrap();
        let q = res.data.active_daily_coding_challenge_question;
        assert_eq!(q.link, "/problems/two-sum/");
        assert_eq!(
            (q.question.id.as_str(), q.question.title_slug.as_str()),
            ("1", "two-sum")
        );
    }

    #[test]
    fn parses_recent_accepted_submissions() {
        let json = r#"{"data":{"recentAcSubmissionList":[
            {"titleSlug":"two-sum","timestamp":"1759700000"},
            {"titleSlug":"3sum","timestamp":"1759600000"}]}}"#;
        let res: SubmissionsResponse = serde_json::from_str(json).unwrap();
        let slugs: Vec<_> = res
            .data
            .recent_ac_submission_list
            .iter()
            .map(|s| s.title_slug.as_str())
            .collect();
        assert_eq!(slugs, ["two-sum", "3sum"]);
    }

    #[test]
    fn a_question_without_an_acceptance_rate_still_parses() {
        let q: Question = serde_json::from_str(
            r#"{"difficulty":"Hard","questionFrontendId":"42","title":"Trapping Rain Water","titleSlug":"trapping-rain-water","acRate":null}"#,
        )
        .unwrap();
        assert_eq!(q.ac_rate, None);
    }
}
