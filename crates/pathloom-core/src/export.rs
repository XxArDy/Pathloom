//! Read-only HTML export (m10 contract A8): one self-contained file, inline CSS, no scripts, no external URLs.

use crate::error::Result;
use crate::layout::{card_ref, card_rect};
use crate::model::{StageView, Status, Task};
use crate::project::Project;
use crate::status::stage_views;
use std::fmt::Write;

/// Escapes text for HTML content and attribute values.
pub fn html_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

const CSS: &str = "body{font:15px/1.5 system-ui,sans-serif;margin:0;padding:24px;background:#f6f6f4;color:#1c1c1a}\
h1{margin:0 0 4px}h2{margin:32px 0 8px;font-size:18px}.meta{color:#666;margin:0 0 16px}\
.stage{background:#fff;border:1px solid #ddd;border-radius:8px;padding:12px 16px;margin:8px 0}\
.stage h3{margin:0;font-size:16px}.key{color:#888;font-weight:400;font-size:13px}\
.bar{height:6px;background:#e6e6e2;border-radius:3px;margin:6px 0;overflow:hidden}.bar i{display:block;height:100%;background:#3a8f5c}\
ul{margin:4px 0;padding-left:20px}ul.tasks{list-style:none;padding-left:0}ul.tasks ul{list-style:none;padding-left:22px}\
.small{color:#666;font-size:13px}svg{max-width:100%;height:auto;background:#fff;border:1px solid #ddd;border-radius:8px}\
svg rect{fill:#fff;stroke:#999}svg .done{fill:#e3f3e8;stroke:#3a8f5c}svg .active{fill:#fff6dc;stroke:#c99a1a}svg .blocked{fill:#f4e4e4;stroke:#b55}\
svg text{font:12px system-ui,sans-serif;fill:#1c1c1a}svg line{stroke:#888;stroke-width:1.5}svg line.soft{stroke-dasharray:5 4}";

fn status_name(s: Status) -> &'static str {
    s.as_str()
}

fn tasks_html(out: &mut String, tasks: &[Task]) {
    out.push_str("<ul class=\"tasks\">");
    for t in tasks {
        let _ = write!(out, "<li><input type=\"checkbox\" disabled{}> {}", if t.is_done() { " checked" } else { "" }, html_escape(&t.title));
        if !t.sub.is_empty() {
            tasks_html(out, &t.sub);
        }
        out.push_str("</li>");
    }
    out.push_str("</ul>");
}

fn stage_html(out: &mut String, v: &StageView, all: &[StageView]) {
    let s = &v.stage;
    let (done, total) = v.progress;
    let pct = (done * 100).checked_div(total).unwrap_or(0);
    let _ = write!(out, "<div class=\"stage\"><h3>{} <span class=\"key\">{}</span></h3>", html_escape(&s.title), html_escape(&s.key));
    let _ = write!(out, "<div class=\"small\">{} · {}", status_name(v.status), html_escape(&s.who.display()));
    if let Some(d) = &s.due {
        let _ = write!(out, " · due {}", html_escape(d));
    }
    let _ = write!(out, "</div><div class=\"bar\"><i style=\"width:{pct}%\"></i></div><div class=\"small\">{done}/{total}</div>");
    if !s.tasks.is_empty() {
        tasks_html(out, &s.tasks);
    }
    if !s.depends.is_empty() {
        out.push_str("<div class=\"small\">Depends on:</div><ul>");
        for d in &s.depends {
            let title = all.iter().find(|o| o.stage.key == d.id).map_or("", |o| o.stage.title.as_str());
            let _ = write!(out, "<li>{} {}{}</li>", html_escape(&d.id), html_escape(title), if d.soft { " (soft)" } else { "" });
        }
        out.push_str("</ul>");
    }
    out.push_str("</div>");
}

impl Project {
    /// The default board (`roadmap`, else the first one) as an SVG: cards, titles and straight dependency lines.
    fn board_svg(&self, views: &[StageView]) -> Result<Option<String>> {
        let boards = self.list_boards()?;
        let Some(name) = boards.iter().find(|b| *b == "roadmap").or(boards.first()) else { return Ok(None) };
        let board = self.load_board(name)?;
        let cards: Vec<(&StageView, f64, f64, f64, f64)> = board
            .elements
            .iter()
            .filter_map(|e| {
                let r = card_ref(e)?;
                let v = views.iter().find(|v| v.stage.key == r)?;
                let r = card_rect(e);
                Some((v, r.x, r.y, r.w, r.h))
            })
            .collect();
        if cards.is_empty() {
            return Ok(None);
        }
        let min = |f: fn(&(&StageView, f64, f64, f64, f64)) -> f64| cards.iter().map(f).fold(f64::INFINITY, f64::min) - 20.0;
        let max = |f: fn(&(&StageView, f64, f64, f64, f64)) -> f64| cards.iter().map(f).fold(f64::NEG_INFINITY, f64::max) + 20.0;
        let (x0, y0) = (min(|c| c.1), min(|c| c.2));
        let (w, h) = (max(|c| c.1 + c.3) - x0, max(|c| c.2 + c.4) - y0);
        let mut o = format!("<svg viewBox=\"{x0} {y0} {w} {h}\" width=\"{w}\" height=\"{h}\">");
        for (v, x, y, _, ch) in &cards {
            for d in &v.stage.depends {
                if let Some((_, fx, fy, fw, fh)) = cards.iter().find(|c| c.0.stage.key == d.id) {
                    let _ = write!(
                        o,
                        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{}/>",
                        fx + fw,
                        fy + fh / 2.0,
                        x,
                        y + ch / 2.0,
                        if d.soft { " class=\"soft\"" } else { "" }
                    );
                }
            }
        }
        for (v, x, y, cw, ch) in &cards {
            let max_chars = (*cw / 7.0) as usize;
            let t: String = v.stage.title.chars().take(max_chars).collect();
            let _ = write!(
                o,
                "<rect class=\"{}\" x=\"{x}\" y=\"{y}\" width=\"{cw}\" height=\"{ch}\" rx=\"8\"/><text x=\"{}\" y=\"{}\">{}</text>",
                status_name(v.status),
                x + 10.0,
                y + 22.0,
                html_escape(&t)
            );
        }
        o.push_str("</svg>");
        Ok(Some(o))
    }

    /// One self-contained, read-only HTML page of the plan; `now` is shown as the export time.
    pub fn export_html(&self, now: &str) -> Result<String> {
        let views = stage_views(&self.load_stages()?);
        let name = html_escape(&self.config.name);
        let mut o = format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
             <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:\">\
             <title>{name}</title><style>{CSS}</style></head><body><h1>{name}</h1><p class=\"meta\">Read-only export · {}</p>",
            html_escape(now)
        );
        if let Some(svg) = self.board_svg(&views)? {
            o.push_str(&svg);
        }
        for (st, label) in [(Status::Active, "Active"), (Status::Ready, "Ready"), (Status::Blocked, "Blocked"), (Status::Done, "Done")] {
            let group: Vec<&StageView> = views.iter().filter(|v| v.status == st).collect();
            if group.is_empty() {
                continue;
            }
            let _ = write!(o, "<h2>{label} ({})</h2>", group.len());
            for v in group {
                stage_html(&mut o, v, &views);
            }
        }
        o.push_str("</body></html>\n");
        Ok(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::{ApplySpec, DepSpec, StageSpec};

    #[test]
    fn escapes_markup() {
        assert_eq!(html_escape(r#"<script>alert("x")&'</script>"#), "&lt;script&gt;alert(&quot;x&quot;)&amp;&#39;&lt;/script&gt;");
    }

    #[test]
    fn export_is_self_contained_and_escaped() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::init(dir.path(), "My <b>plan</b>").unwrap();
        let st = |k: &str, t: &str, deps: &[&str]| StageSpec {
            id: Some(k.into()),
            title: Some(t.into()),
            depends: Some(deps.iter().map(|d| DepSpec::Key((*d).into())).collect()),
            tasks: vec!["do <i>it</i>".into()],
            ..Default::default()
        };
        let spec = ApplySpec { stages: vec![st("A", "<script>alert(1)</script>", &[]), st("B", "Second", &["A"])], ..Default::default() };
        p.apply(&spec, &Default::default()).unwrap();
        let html = p.export_html("2026-11-20T10:00:00Z").unwrap();
        for bad in ["<script", "<b>plan", "<i>it", "http://", "https://"] {
            assert!(!html.contains(bad), "{bad}");
        }
        assert!(html.contains("&lt;script&gt;alert(1)") && html.contains("My &lt;b&gt;plan"));
        assert!(html.contains("<svg") && html.contains("<line") && html.contains("disabled") && html.contains("Blocked (1)"));
        assert!(html.contains("2026-11-20T10:00:00Z"));
        assert!(html.contains("Content-Security-Policy\" content=\"default-src 'none'"), "no scripts or remote loads even if escaping fails");
    }
}
