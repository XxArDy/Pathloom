//! Quick add in plain words (m10 contract A2): a rule-based parser, no network or models.
//! Understands English and Ukrainian: quoted title, stage (`в етап X`, `to stage X`, `in X`), due date
//! (`завтра`, `до п'ятниці`, `by friday`, `12.12`, `2026-12-12`) and assignee (`мені`, `агенту`, `for me`, `for ai`).

use crate::agenda::{date_to_days, days_to_date};
use crate::model::Who;
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Draft {
    pub title: String,
    /// Key of the matched stage.
    pub stage: Option<String>,
    /// `YYYY-MM-DD`.
    pub due: Option<String>,
    pub who: Option<Who>,
    /// Phrases that looked like a stage or a date but could not be resolved (left out of the title).
    pub unmatched: Vec<String>,
}

const QUOTES: [(char, char); 4] = [('"', '"'), ('«', '»'), ('“', '”'), ('„', '“')];
// Longest first: the first command that fits is stripped.
const COMMANDS: [&str; 15] = [
    "додай задачу", "додати задачу", "додай таск", "створи задачу", "нова задача", "add a task", "add task", "new task",
    "create task", "додай", "додати", "add", "задачу", "задача", "task",
];
const TRAILING: [&str; 12] = ["в", "у", "to", "in", "до", "by", "на", "для", "for", "on", "and", "і"];
const PREFIX: [&str; 5] = ["до", "by", "on", "на", "before"];
const WEEKDAYS: [&[&str]; 7] = [
    &["понеділок", "понеділка", "понеділку", "monday"],
    &["вівторок", "вівторка", "вівторку", "tuesday"],
    &["середа", "середи", "середу", "wednesday"],
    &["четвер", "четверга", "четвергу", "thursday"],
    &["пятниця", "пятниці", "пятницю", "friday"],
    &["субота", "суботи", "суботу", "saturday"],
    &["неділя", "неділі", "неділю", "sunday"],
];
const ENDINGS: [&str; 13] = ["ою", "ею", "ом", "ем", "ах", "ях", "а", "и", "і", "у", "ю", "я", "ї"];
const MARKERS: [&[&str]; 11] = [
    &["в", "етап"], &["в", "етапі"], &["у", "етап"], &["у", "етапі"], &["до", "етапу"], &["to", "stage"],
    &["in", "stage"], &["to"], &["in"], &["в"], &["у"],
];

/// Lowercase, without apostrophes and edge punctuation.
fn key_of(t: &str) -> String {
    t.trim_matches(|c: char| ",;:!?".contains(c))
        .trim_end_matches('.')
        .chars()
        .filter(|c| !"'ʼ’`".contains(*c))
        .collect::<String>()
        .to_lowercase()
}

/// A word without a Ukrainian case ending (words longer than 4 letters only).
fn stem(w: &str) -> String {
    if w.chars().count() > 4 {
        for e in ENDINGS {
            if let Some(s) = w.strip_suffix(e)
                && s.chars().count() >= 4
            {
                return s.into();
            }
        }
    }
    w.into()
}

fn norm(s: &str) -> String {
    s.split_whitespace().map(|w| stem(&key_of(w))).collect::<Vec<_>>().join(" ")
}

/// 4 = key, 3 = same title, 2 = title starts with the phrase, 1 = title contains it, 0 = no match.
fn score(phrase: &str, key: &str, title: &str) -> u8 {
    if phrase.eq_ignore_ascii_case(key) {
        return 4;
    }
    let (p, t) = (norm(phrase), norm(title));
    if p.chars().count() < 2 {
        0
    } else if p == t {
        3
    } else if t.starts_with(&p) {
        2
    } else if p.chars().count() >= 3 && t.contains(&p) {
        1
    } else {
        0
    }
}

enum Dt {
    Ok(i64),
    Bad,
}

/// Date meaning of one token, if it has one.
fn date_of(tok: &str, today: i64) -> Option<Dt> {
    let k = key_of(tok);
    match k.as_str() {
        "сьогодні" | "today" => return Some(Dt::Ok(today)),
        "завтра" | "tomorrow" => return Some(Dt::Ok(today + 1)),
        "післязавтра" => return Some(Dt::Ok(today + 2)),
        _ => {}
    }
    if let Some(wd) = WEEKDAYS.iter().position(|f| f.contains(&k.as_str())) {
        // Next occurrence, strictly after today. 1970-01-01 was a Thursday; Monday = 0.
        let ahead = (wd as i64 - (today + 3).rem_euclid(7)).rem_euclid(7);
        return Some(Dt::Ok(today + if ahead == 0 { 7 } else { ahead }));
    }
    if let Some(v) = date_to_days(&k) {
        return Some(Dt::Ok(v));
    }
    let digits = |s: &str, n: usize| !s.is_empty() && s.len() <= n && s.bytes().all(|b| b.is_ascii_digit());
    let year = days_to_date(today)[..4].parse::<i64>().unwrap_or(1970);
    match k.split('.').collect::<Vec<_>>().as_slice() {
        [d, m] if digits(d, 2) && digits(m, 2) => {
            let at = |y: i64| date_to_days(&format!("{y:04}-{m:0>2}-{d:0>2}"));
            Some(match at(year) {
                Some(v) if v >= today => Dt::Ok(v),
                Some(_) => at(year + 1).map_or(Dt::Bad, Dt::Ok),
                None => Dt::Bad,
            })
        }
        [d, m, y] if digits(d, 2) && digits(m, 2) && y.len() == 4 && digits(y, 4) => {
            Some(date_to_days(&format!("{y}-{m:0>2}-{d:0>2}")).map_or(Dt::Bad, Dt::Ok))
        }
        _ => None,
    }
}

fn who_of(k: &str) -> Option<Who> {
    match k {
        "мені" | "me" | "мене" => Some(Who::me()),
        "агенту" | "агентові" | "агента" | "ai" | "ші" => Some(Who::ai()),
        _ => None,
    }
}

/// Parses `text`. `stages` are `(key, title)` pairs, `today` is `YYYY-MM-DD` (an invalid one disables dates).
pub fn parse(text: &str, stages: &[(String, String)], today: &str) -> Draft {
    let mut d = Draft::default();
    // 1. Quoted title; its content is not parsed further.
    let mut rest = text.to_string();
    let mut quoted = None;
    for (o, c) in QUOTES {
        if let Some(a) = rest.find(o) {
            let from = a + o.len_utf8();
            if let Some(b) = rest[from..].find(c) {
                quoted = Some(rest[from..from + b].trim().to_string());
                rest.replace_range(a..from + b + c.len_utf8(), " ");
                break;
            }
        }
    }
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let keys: Vec<String> = toks.iter().map(|t| key_of(t)).collect();
    let mut used = vec![false; toks.len()];

    // 2. Who: `for me`, `для мене`, `мені`, `агенту`.
    for i in 0..toks.len() {
        let after_for = matches!(keys[i].as_str(), "for" | "для") && i + 1 < toks.len();
        if after_for && let Some(w) = who_of(&keys[i + 1]) {
            d.who = Some(w);
            used[i] = true;
            used[i + 1] = true;
            break;
        }
        if keys[i] != "мене" && let Some(w) = who_of(&keys[i]) {
            d.who = Some(w);
            used[i] = true;
            break;
        }
    }

    // 3. Due date, optionally after `до`/`by`.
    if let Some(t) = date_to_days(today) {
        'due: for i in 0..toks.len() {
            if used[i] {
                continue;
            }
            let with_prefix = PREFIX.contains(&keys[i].as_str()) && i + 1 < toks.len() && !used[i + 1];
            for at in if with_prefix { vec![i + 1, i] } else { vec![i] } {
                match date_of(toks[at], t) {
                    Some(Dt::Ok(v)) => d.due = Some(days_to_date(v)),
                    Some(Dt::Bad) => d.unmatched.push(toks[at].trim_matches(|c: char| ",;:!?".contains(c)).into()),
                    None => continue,
                }
                used[at] = true;
                used[i] = true;
                break 'due;
            }
        }
    }

    // 4. Stage after a marker; the longest following phrase that names a stage wins.
    'stage: for i in 0..toks.len() {
        for m in MARKERS {
            let end = i + m.len();
            if end > toks.len() || (i..end).any(|j| used[j]) || !m.iter().enumerate().all(|(j, w)| keys[i + j] == *w) {
                continue;
            }
            let avail = (end..toks.len()).take_while(|&j| !used[j]).count().min(6);
            let best = (1..=avail).rev().find_map(|l| {
                let phrase = toks[end..end + l].join(" ");
                stages
                    .iter()
                    .map(|(k, t)| (score(&phrase, k, t), k))
                    .filter(|(s, _)| *s > 0)
                    .max_by_key(|(s, _)| *s)
                    .map(|(_, k)| (l, k.clone()))
            });
            if let Some((l, k)) = best {
                d.stage = Some(k);
                (i..end + l).for_each(|j| used[j] = true);
                break 'stage;
            }
            // `в етап <word>` / `to stage <word>` that names nothing: report it. Bare `in`/`to`/`в` stay in the title.
            if m.len() == 2 && end < toks.len() {
                d.unmatched.push(toks[end].into());
                (i..=end).for_each(|j| used[j] = true);
                break 'stage;
            }
        }
    }

    // 5. Title: the quoted text, else what is left without command words.
    d.title = match quoted {
        Some(q) if !q.is_empty() => q,
        _ => title_from(&toks, &used),
    };
    d
}

fn title_from(toks: &[&str], used: &[bool]) -> String {
    let mut words: Vec<&str> = toks.iter().zip(used).filter(|(_, u)| !**u).map(|(t, _)| *t).collect();
    loop {
        let low: Vec<String> = words.iter().map(|w| w.trim_end_matches(':').to_lowercase()).collect();
        let cmd = COMMANDS.iter().map(|c| (c, c.split(' ').count())).find(|(c, n)| low.len() >= *n && low[..*n].join(" ") == **c);
        if let Some((_, n)) = cmd {
            words.drain(..n);
        } else if words.last().is_some_and(|w| TRAILING.contains(&key_of(w).as_str())) {
            words.pop();
        } else {
            break;
        }
    }
    words.join(" ").trim_matches(|c: char| ":,-;. ".contains(c)).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-20 is a Friday.
    const TODAY: &str = "2026-11-20";

    fn st() -> Vec<(String, String)> {
        [("AUTH", "Авторизація"), ("UI", "Інтерфейс користувача"), ("API", "Backend API"), ("DOCS", "Documentation")]
            .iter()
            .map(|(k, t)| (k.to_string(), t.to_string()))
            .collect()
    }

    fn p(t: &str) -> Draft {
        parse(t, &st(), TODAY)
    }

    #[test]
    fn titles() {
        for (t, want) in [
            (r#"Додай задачу "Зробити логін" в етап AUTH"#, "Зробити логін"),
            ("add task «Fix bug» to stage API", "Fix bug"),
            ("“Write docs” in DOCS", "Write docs"),
            ("додай задачу написати тести в етап AUTH", "написати тести"),
            ("add task write tests to stage API", "write tests"),
            ("Додати задачу: полірувати кнопки", "полірувати кнопки"),
            ("fix the parser", "fix the parser"),
        ] {
            assert_eq!(p(t).title, want, "{t}");
        }
    }

    #[test]
    fn stages() {
        for (t, want) in [
            ("add task x to stage AUTH", Some("AUTH")),
            ("add task x to stage auth", Some("AUTH")),
            ("add task x in docs", Some("DOCS")),
            ("add task x in documentation", Some("DOCS")),
            ("додай x в етап авторизація", Some("AUTH")),
            ("додай x в етапі авторизації", Some("AUTH")),
            ("додай x в етап Інтерфейс користувача", Some("UI")),
            ("додай x в етап інтерфейсі", Some("UI")),
            ("додай x в етап backend", Some("API")),
            ("додай x у етап API", Some("API")),
            ("додай x в авторизацію", Some("AUTH")),
            ("fix bug in parser", None),
            ("add x to stage nothing", None),
        ] {
            assert_eq!(p(t).stage.as_deref(), want, "{t}");
        }
        assert_eq!(p("add task x to stage nothing").unmatched, vec!["nothing"]);
        assert_eq!(p("fix bug in parser").title, "fix bug in parser");
    }

    #[test]
    fn dates() {
        for (t, want) in [
            ("x сьогодні", Some("2026-11-20")),
            ("x today", Some("2026-11-20")),
            ("x завтра", Some("2026-11-21")),
            ("x tomorrow", Some("2026-11-21")),
            ("x післязавтра", Some("2026-11-22")),
            ("x до понеділка", Some("2026-11-23")),
            ("x до п'ятниці", Some("2026-11-27")),
            ("x до пʼятниці", Some("2026-11-27")),
            ("x by friday", Some("2026-11-27")),
            ("x by sunday", Some("2026-11-22")),
            ("x до середи", Some("2026-11-25")),
            ("x by saturday", Some("2026-11-21")),
            ("x 12.12", Some("2026-12-12")),
            ("x до 12.12", Some("2026-12-12")),
            ("x 05.01", Some("2027-01-05")),
            ("x 20.11", Some("2026-11-20")),
            ("x 12.12.2027", Some("2027-12-12")),
            ("x 2026-12-31", Some("2026-12-31")),
            ("x by 2026-12-31", Some("2026-12-31")),
            ("x 31.02", None),
            ("x nothing", None),
        ] {
            assert_eq!(p(t).due.as_deref(), want, "{t}");
        }
        assert_eq!(p("x 31.02").unmatched, vec!["31.02"]);
        assert_eq!(p("write docs by friday").title, "write docs");
        assert_eq!(p("написати тести до п'ятниці").title, "написати тести");
    }

    #[test]
    fn who() {
        for (t, want) in [
            ("зроби x мені", Some("me")),
            ("x для мене", Some("me")),
            ("x for me", Some("me")),
            ("x агенту", Some("ai")),
            ("x for ai", Some("ai")),
            ("x ai", Some("ai")),
            ("x для агента", Some("ai")),
            ("x", None),
        ] {
            assert_eq!(p(t).who.map(|w| w.to_string()).as_deref(), want, "{t}");
        }
        assert_eq!(p("write docs for me").title, "write docs");
    }

    #[test]
    fn everything_together() {
        let d = p(r#"Додай задачу "Підключити OAuth" в етап авторизація до п'ятниці мені"#);
        assert_eq!(
            d,
            Draft {
                title: "Підключити OAuth".into(),
                stage: Some("AUTH".into()),
                due: Some("2026-11-27".into()),
                who: Some(Who::me()),
                unmatched: vec![]
            }
        );
        let d = p("add task review the API to stage api by monday for ai");
        assert_eq!((d.title.as_str(), d.stage.as_deref(), d.due.as_deref()), ("review the API", Some("API"), Some("2026-11-23")));
        assert_eq!(d.who, Some(Who::ai()));
    }

    #[test]
    fn quoted_text_is_not_parsed() {
        let d = p(r#"add "ship it tomorrow in AUTH" to stage docs"#);
        assert_eq!((d.title.as_str(), d.stage.as_deref(), d.due), ("ship it tomorrow in AUTH", Some("DOCS"), None));
    }

    #[test]
    fn bad_today_disables_dates() {
        assert_eq!(parse("x tomorrow", &st(), "nope").due, None);
    }
}
