//! Fuzzy search over places and keywords, and a light natural-language read
//! of what the user typed ("is it raining in Paris?", "weather in Tucson
//! tomorrow", "radar", "alerts near me").
//!
//! Everything here is pure and unit tested; the app feeds it the query text,
//! saved places and geocoder results and gets ranked suggestions back.

/// Case-insensitive fuzzy score of `query` against `candidate`.
///
/// A subsequence match scores by how compact it is, with bonuses for a
/// prefix, word-start hits and consecutive runs. When there is no subsequence
/// match, a typo-tolerant pass compares the query with each word of the
/// candidate (one edit allowed from 4 letters, two from 8). `None` = no match.
pub fn score(query: &str, candidate: &str) -> Option<i32> {
    let q: Vec<char> = query.trim().to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    let c: Vec<char> = candidate.to_lowercase().chars().collect();
    if let Some(s) = subsequence_score(&q, &c) {
        return Some(s);
    }
    typo_score(&q, &c)
}

fn is_boundary(c: &[char], i: usize) -> bool {
    i == 0 || !c[i - 1].is_alphanumeric()
}

fn subsequence_score(q: &[char], c: &[char]) -> Option<i32> {
    let mut score = 0i32;
    let mut qi = 0;
    let mut last: Option<usize> = None;
    let mut first: Option<usize> = None;
    for (ci, ch) in c.iter().enumerate() {
        if qi < q.len() && *ch == q[qi] {
            score += 10;
            if is_boundary(c, ci) {
                score += 15;
            }
            if let Some(l) = last {
                if l + 1 == ci {
                    score += 12;
                } else {
                    score -= ((ci - l - 1) as i32).min(8);
                }
            }
            first.get_or_insert(ci);
            last = Some(ci);
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    if first == Some(0) {
        score += 25;
    }
    // Shorter candidates win ties ("Paris" over "Paris, Tennessee").
    score -= (c.len() as i32 - q.len() as i32).max(0) / 4;
    Some(score)
}

/// Optimal-string-alignment (Damerau–Levenshtein) distance: insertions,
/// deletions, substitutions and adjacent transpositions each cost 1. Capped
/// at `max + 1` for speed.
pub fn edit_distance(a: &[char], b: &[char], max: usize) -> usize {
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    let w = b.len() + 1;
    let mut rows = vec![vec![0usize; w]; a.len() + 1];
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        rows[i][0] = i;
        let mut row_min = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (rows[i - 1][j - 1] + cost)
                .min(rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = v;
            row_min = row_min.min(v);
        }
        if row_min > max {
            return max + 1;
        }
    }
    rows[a.len()][b.len()]
}

fn typo_score(q: &[char], c: &[char]) -> Option<i32> {
    let allowed = match q.len() {
        0..=3 => return None,
        4..=7 => 1,
        _ => 2,
    };
    c.split(|ch| !ch.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .filter_map(|w| {
            // Compare against the word, and against its prefix of the same
            // length so "chicgo" still finds "Chicago Heights".
            let whole = edit_distance(q, w, allowed);
            let prefix = if w.len() > q.len() {
                edit_distance(q, &w[..q.len()], allowed)
            } else {
                allowed + 1
            };
            let d = whole.min(prefix);
            (d <= allowed).then(|| 5 - 4 * d as i32)
        })
        .max()
}

/// What a typed query is asking for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Jump to a view.
    View(View),
    Refresh,
    Settings,
    UseMyLocation,
    Units(Units),
    /// Write (or ask the AI for) a summary of the active forecast.
    Summarize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Forecast,
    Hourly,
    Week,
    Radar,
    Alerts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    Celsius,
    Fahrenheit,
}

/// A keyword action shown in the search dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyword {
    pub title: &'static str,
    /// Extra words it should also match.
    pub aliases: &'static [&'static str],
    pub intent: Intent,
}

pub fn keywords() -> Vec<Keyword> {
    use Intent as I;
    vec![
        Keyword {
            title: "Forecast",
            aliases: &["today", "now", "current", "conditions", "home"],
            intent: I::View(View::Forecast),
        },
        Keyword {
            title: "Hourly forecast",
            aliases: &["hours", "hour by hour", "next hours"],
            intent: I::View(View::Hourly),
        },
        Keyword {
            title: "7-day forecast",
            aliases: &["week", "weekly", "daily", "seven day", "10 day"],
            intent: I::View(View::Week),
        },
        Keyword {
            title: "Radar",
            aliases: &["map", "rain map", "precipitation", "storms"],
            intent: I::View(View::Radar),
        },
        Keyword {
            title: "Weather alerts",
            aliases: &["warnings", "watches", "advisories", "wmo", "severe"],
            intent: I::View(View::Alerts),
        },
        Keyword {
            title: "Summarize the forecast",
            aliases: &["summary", "summarise", "tl;dr", "overview", "ai"],
            intent: I::Summarize,
        },
        Keyword {
            title: "Refresh",
            aliases: &["reload", "update"],
            intent: I::Refresh,
        },
        Keyword {
            title: "Use my location",
            aliases: &["near me", "here", "locate", "gps"],
            intent: I::UseMyLocation,
        },
        Keyword {
            title: "Switch to Celsius",
            aliases: &["°c", "metric", "c"],
            intent: I::Units(Units::Celsius),
        },
        Keyword {
            title: "Switch to Fahrenheit",
            aliases: &["°f", "imperial", "f"],
            intent: I::Units(Units::Fahrenheit),
        },
        Keyword {
            title: "Settings",
            aliases: &["preferences", "options", "api key", "units", "theme"],
            intent: I::Settings,
        },
    ]
}

/// Best fuzzy score of a keyword against the query (title or any alias).
pub fn keyword_score(query: &str, k: &Keyword) -> Option<i32> {
    let q = query.trim();
    // Very short queries only match aliases exactly ("c", "f") or title prefixes.
    if q.chars().count() <= 2 {
        let ql = q.to_lowercase();
        if k.aliases.iter().any(|a| *a == ql) {
            return Some(100);
        }
        return k.title.to_lowercase().starts_with(&ql).then_some(40);
    }
    std::iter::once(k.title)
        .chain(k.aliases.iter().copied())
        .filter_map(|c| score(q, c))
        .max()
}

/// Ranked `(index, score)` of `candidates` matching `query`, best first.
pub fn rank<'a, I>(query: &str, candidates: I) -> Vec<(usize, i32)>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut out: Vec<(usize, i32)> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(i, c)| score(query, c).map(|s| (i, s)))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// Which day a question is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayRef {
    Today,
    Tomorrow,
    /// 0 = Monday … 6 = Sunday.
    Weekday(u32),
}

/// The topic of a weather question, for quick rule-based answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Rain,
    Snow,
    Umbrella,
    Wind,
    Hot,
    Cold,
    Storm,
    Jacket,
    General,
}

/// A natural-language reading of the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The place named in the query, if any ("Paris", "the Eiffel Tower").
    pub place: Option<String>,
    pub day: Option<DayRef>,
    pub topic: Option<Topic>,
    /// Reads as a question ("is it…", "will it…", ends with "?").
    pub is_question: bool,
}

const QUESTION_STARTS: &[&str] = &[
    "is ", "will ", "should ", "what", "when", "how", "do ", "does ", "can ", "are ", "any ",
];

/// Phrases dropped before looking for a place name, longest first.
const FILLER: &[&str] = &[
    "what's the weather like",
    "what is the weather like",
    "what's the weather",
    "what is the weather",
    "how's the weather",
    "how is the weather",
    "do i need an umbrella",
    "do i need a jacket",
    "should i bring an umbrella",
    "should i bring a jacket",
    "is it going to rain",
    "is it going to snow",
    "will it rain",
    "will it snow",
    "is it raining",
    "is it snowing",
    "is it windy",
    "is it hot",
    "is it cold",
    "are there storms",
    "any storms",
    "any alerts",
    "forecast for",
    "weather for",
    "weather in",
    "weather at",
    "forecast in",
    "forecast",
    "weather",
    "radar",
    "alerts",
    "the rest of the week",
    "this week",
    "this weekend",
    "right now",
    "today",
    "tonight",
    "tomorrow",
];

const PREPOSITIONS: &[&str] = &["in", "at", "near", "for", "around", "on", "over"];

const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

fn detect_day(q: &str) -> Option<DayRef> {
    if q.contains("tomorrow") {
        return Some(DayRef::Tomorrow);
    }
    for (i, d) in WEEKDAYS.iter().enumerate() {
        if contains_word(q, d) || contains_word(q, &d[..3]) {
            return Some(DayRef::Weekday(i as u32));
        }
    }
    if q.contains("today") || q.contains("tonight") || q.contains("right now") {
        return Some(DayRef::Today);
    }
    None
}

fn contains_word(hay: &str, word: &str) -> bool {
    hay.split(|c: char| !c.is_alphanumeric()).any(|w| w == word)
}

fn detect_topic(q: &str) -> Option<Topic> {
    let has = |ws: &[&str]| ws.iter().any(|w| q.contains(w));
    if has(&["umbrella"]) {
        Some(Topic::Umbrella)
    } else if has(&["jacket", "coat", "sweater"]) {
        Some(Topic::Jacket)
    } else if has(&["thunder", "storm", "lightning", "tornado"]) {
        Some(Topic::Storm)
    } else if has(&["snow", "flurr", "blizzard", "sleet"]) {
        Some(Topic::Snow)
    } else if has(&["rain", "shower", "drizzle", "wet", "pour"]) {
        Some(Topic::Rain)
    } else if has(&["wind", "gust", "breez"]) {
        Some(Topic::Wind)
    } else if has(&["hot", "heat", "warm"]) {
        Some(Topic::Hot)
    } else if has(&["cold", "freez", "chilly", "frost"]) {
        Some(Topic::Cold)
    } else if has(&["weather", "forecast", "like"]) {
        Some(Topic::General)
    } else {
        None
    }
}

/// Read a query as natural language. `place` is whatever is left after
/// removing question/filler phrases, day words and a leading preposition.
pub fn parse(query: &str) -> Parsed {
    let raw = query.trim();
    let mut q = raw.to_lowercase();
    let is_question = raw.ends_with('?') || QUESTION_STARTS.iter().any(|s| q.starts_with(s));
    let day = detect_day(&q);
    let topic = detect_topic(&q);

    q = q.replace(['?', '!'], " ");
    for f in FILLER {
        q = q.replace(f, " ");
    }
    let mut words: Vec<&str> = q
        .split_whitespace()
        .filter(|w| !WEEKDAYS.contains(w) && !WEEKDAYS.iter().any(|d| &d[..3] == *w))
        .collect();
    while words.first().is_some_and(|w| PREPOSITIONS.contains(w)) {
        words.remove(0);
    }
    while words
        .last()
        .is_some_and(|w| PREPOSITIONS.contains(w) || *w == "on")
    {
        words.pop();
    }
    // "me"/"here" means the current place, not a place named "me".
    let place_lc = words.join(" ");
    let place = if place_lc.is_empty() || matches!(place_lc.as_str(), "me" | "here" | "outside") {
        None
    } else {
        // Recover the user's own capitalisation from the raw query.
        Some(restore_case(raw, &place_lc))
    };
    Parsed {
        place,
        day,
        topic,
        is_question,
    }
}

/// Find `needle_lc` in `raw` case-insensitively and return that slice.
fn restore_case(raw: &str, needle_lc: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.len() == raw.len() {
        if let Some(i) = lower.find(needle_lc) {
            return raw[i..i + needle_lc.len()].to_string();
        }
    }
    title_case(needle_lc)
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_prefers_prefix_and_word_starts() {
        let a = score("san", "San Diego").unwrap();
        let b = score("san", "Pleasanton").unwrap();
        assert!(a > b, "{a} vs {b}");
        let c = score("sd", "San Diego").unwrap();
        assert!(c > score("sd", "Sudan").unwrap());
    }

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(score("", "Seattle"), Some(0));
        assert_eq!(score("   ", "Seattle"), Some(0));
    }

    #[test]
    fn no_match_returns_none() {
        assert_eq!(score("xyz", "Seattle"), None);
        assert_eq!(score("tok", "Norman"), None);
    }

    #[test]
    fn typos_are_tolerated() {
        assert!(score("seatle", "Seattle").is_some()); // subsequence
        assert!(score("chicgo", "Chicago").is_some());
        assert!(score("nromal", "Norman").is_none());
        assert!(score("nrman", "Norman").is_some());
        assert!(score("pairs", "Paris").is_some()); // one transposition
    }

    #[test]
    fn edit_distance_basics() {
        let d = |a: &str, b: &str| {
            edit_distance(
                &a.chars().collect::<Vec<_>>(),
                &b.chars().collect::<Vec<_>>(),
                5,
            )
        };
        assert_eq!(d("kitten", "sitting"), 3);
        assert_eq!(d("", "abc"), 3);
        assert_eq!(d("same", "same"), 0);
        assert_eq!(d("pairs", "paris"), 1);
    }

    #[test]
    fn rank_orders_best_first() {
        let names = ["Seattle", "San Diego", "Santa Fe", "Norman"];
        let r = rank("sa", names.iter().copied());
        let order: Vec<&str> = r.iter().map(|(i, _)| names[*i]).collect();
        assert_eq!(order.first(), Some(&"San Diego"));
        assert!(!order.contains(&"Norman"));
    }

    #[test]
    fn keywords_match_titles_and_aliases() {
        let ks = keywords();
        let find = |q: &str| {
            ks.iter()
                .filter_map(|k| keyword_score(q, k).map(|s| (s, k.intent.clone())))
                .max_by_key(|(s, _)| *s)
                .map(|(_, i)| i)
        };
        assert_eq!(find("radr"), Some(Intent::View(View::Radar)));
        assert_eq!(find("warnings"), Some(Intent::View(View::Alerts)));
        assert_eq!(find("summ"), Some(Intent::Summarize));
        assert_eq!(find("c"), Some(Intent::Units(Units::Celsius)));
        assert_eq!(find("week"), Some(Intent::View(View::Week)));
    }

    #[test]
    fn parses_place_and_day() {
        let p = parse("weather in Paris tomorrow");
        assert_eq!(p.place.as_deref(), Some("Paris"));
        assert_eq!(p.day, Some(DayRef::Tomorrow));
        assert!(!p.is_question);

        let p = parse("Is it raining at the Eiffel Tower?");
        assert_eq!(p.place.as_deref(), Some("the Eiffel Tower"));
        assert_eq!(p.topic, Some(Topic::Rain));
        assert!(p.is_question);

        let p = parse("will it snow in Denver on Friday");
        assert_eq!(p.place.as_deref(), Some("Denver"));
        assert_eq!(p.day, Some(DayRef::Weekday(4)));
        assert_eq!(p.topic, Some(Topic::Snow));
    }

    #[test]
    fn plain_place_queries_pass_through() {
        let p = parse("San Diego");
        assert_eq!(p.place.as_deref(), Some("San Diego"));
        assert_eq!(p.day, None);
        assert_eq!(p.topic, None);
    }

    #[test]
    fn questions_about_here_have_no_place() {
        let p = parse("do I need an umbrella today?");
        assert_eq!(p.place, None);
        assert_eq!(p.topic, Some(Topic::Umbrella));
        assert_eq!(p.day, Some(DayRef::Today));
        let p = parse("is it windy near me");
        assert_eq!(p.place, None);
        assert_eq!(p.topic, Some(Topic::Wind));
    }

    #[test]
    fn coordinates_survive_parsing() {
        let p = parse("weather at 35.22, -97.44");
        assert_eq!(p.place.as_deref(), Some("35.22, -97.44"));
    }
}
