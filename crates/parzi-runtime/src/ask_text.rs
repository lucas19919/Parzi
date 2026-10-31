use serde::Serialize;
use serde_json::Value;

use crate::tools::{category, display_name, edit_paths, shell_command};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailKind {
    Command,
    Paths,
    Url,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AskText {
    pub action: String,
    pub detail: String,
    pub detail_kind: DetailKind,
    pub note: String,
}

#[must_use]
pub fn makes_media(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("image_gen")
        || n.starts_with("image_edit")
        || n.starts_with("generate_image")
        || n.ends_with("_to_video")
        || n.starts_with("video_gen")
        || n.starts_with("generate_video")
}

fn text(args: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| args.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

fn base(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|b| !b.is_empty())
        .unwrap_or(path)
}

fn quoted(title: &str) -> Option<String> {
    let start = title.find('`')? + 1;
    let end = start + title[start..].rfind('`')?;
    Some(title[start..end].trim().to_string()).filter(|s| !s.is_empty())
}

#[must_use]
pub fn ask_text(name: &str, args: &Value) -> AskText {
    let shown = display_name(name);
    let title = text(args, &["title"]);
    let note = if args.get("outside").and_then(Value::as_bool) == Some(true) {
        "Outside this session's folder.".to_string()
    } else {
        String::new()
    };
    let ask = |action: String, detail: String, detail_kind| AskText {
        action,
        detail,
        detail_kind,
        note: note.clone(),
    };
    if let Some(cmd) = shell_command(&shown, args).or_else(|| {
        (category(&shown) == Some("shell.exec")).then(|| quoted(&title).unwrap_or_default())
    }) {
        return ask("run a command".into(), cmd, DetailKind::Command);
    }
    if category(&shown) == Some("fs.write") {
        let mut paths = edit_paths(&shown, args);
        if paths.is_empty() {
            paths = args
                .get("paths")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
        }
        let verb = match shown.as_str() {
            "delete" => "delete",
            "move" => "move",
            "Write" | "fs.write" => "write",
            _ => "edit",
        };
        let action = match paths.as_slice() {
            [] => format!("{verb} files"),
            [one] => format!("{verb} {}", base(one)),
            many => format!("{verb} {} files", many.len()),
        };
        return ask(action, paths.join("\n"), DetailKind::Paths);
    }
    if category(&shown) == Some("fs.read") {
        let path = text(args, &["file_path", "path", "pattern"]);
        return ask("read files".into(), path, DetailKind::Paths);
    }
    if makes_media(&shown) {
        let what = if shown.to_lowercase().contains("video") {
            "make a video"
        } else {
            "make a picture"
        };
        return ask(
            what.into(),
            text(args, &["prompt", "description"]),
            DetailKind::Text,
        );
    }
    if let Some((action, detail, kind)) = named(&shown, args) {
        return ask(action, detail, kind);
    }
    if shown.starts_with("browser.") {
        let label = crate::tools::humanize_tool_call(&shown, args);
        return ask("use the open page".into(), label, DetailKind::Text);
    }
    if shown.starts_with("desk.") {
        let label = crate::tools::humanize_tool_call(&shown, args);
        return ask("use this window".into(), label, DetailKind::Text);
    }
    let mut parts = shown.rsplit("__");
    let tool = parts.next().unwrap_or(&shown);
    let server = parts.next().filter(|s| *s != "mcp");
    if tool == "other" || tool.is_empty() {
        return ask("use a tool".into(), title, DetailKind::Text);
    }
    let words = tool
        .rsplit('.')
        .next()
        .unwrap_or(tool)
        .replace(['_', '-'], " ");
    let action = match server {
        Some(s) => format!("use {words} ({s})"),
        None => format!("use {words}"),
    };
    let detail = match text(
        args,
        &["query", "prompt", "title", "message", "path", "url", "id"],
    ) {
        d if !d.is_empty() => d,
        _ => compact_args(args),
    };
    ask(action, detail, DetailKind::Text)
}

fn named(shown: &str, args: &Value) -> Option<(String, String, DetailKind)> {
    let url = text(args, &["url", "source"]);
    if matches!(shown, "WebFetch" | "fetch" | "browser.open") && url.starts_with("http") {
        return Some(("open a web page".into(), url, DetailKind::Url));
    }
    let (action, key, kind) = match shown {
        "WebSearch" | "web_search" => ("search the web", "query", DetailKind::Text),
        "github.issue" => ("file a GitHub issue", "title", DetailKind::Text),
        "project.create" => ("create a project", "title", DetailKind::Text),
        "project.switch" => ("move this session to a project", "slug", DetailKind::Text),
        "project.archive" => ("archive a project", "slug", DetailKind::Text),
        "project.delete" => ("delete a project", "slug", DetailKind::Text),
        "brain.write" => ("write a note", "path", DetailKind::Paths),
        "brain.delete" => ("delete a note", "path", DetailKind::Paths),
        "session.spawn" => ("start a helper session", "title", DetailKind::Text),
        "session.send" => ("message another session", "message", DetailKind::Text),
        _ => return None,
    };
    Some((action.into(), text(args, &[key]), kind))
}

fn compact_args(args: &Value) -> String {
    let Some(o) = args.as_object() else {
        return String::new();
    };
    let shown: serde_json::Map<String, Value> = o
        .iter()
        .filter(|(k, _)| !matches!(k.as_str(), "title" | "outside"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if shown.is_empty() {
        return String::new();
    }
    parzi_core::text::clip(&Value::Object(shown).to_string(), 240)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn say(name: &str, args: Value) -> (String, String) {
        let a = ask_text(name, &args);
        (a.action, a.detail)
    }

    #[test]
    fn commands_files_and_pictures_read_as_what_they_do() {
        assert_eq!(
            say("Bash", json!({"command": "cargo test"})),
            ("run a command".into(), "cargo test".into())
        );
        assert_eq!(
            say("execute", json!({"title": "Execute `npm run build`"})),
            ("run a command".into(), "npm run build".into())
        );
        assert_eq!(
            say("Edit", json!({"file_path": "src/main.rs"})),
            ("edit main.rs".into(), "src/main.rs".into())
        );
        assert_eq!(
            say("edit", json!({"paths": ["a.rs", "b.rs"]})).0,
            "edit 2 files"
        );
        assert_eq!(
            say("image_gen", json!({"prompt": "A red apple on white"})),
            ("make a picture".into(), "A red apple on white".into())
        );
        assert_eq!(
            say("reference_to_video", json!({"prompt": "push in"})).0,
            "make a video"
        );
        assert_eq!(
            say("WebFetch", json!({"url": "https://example.com"})),
            ("open a web page".into(), "https://example.com".into())
        );
    }

    #[test]
    fn connectors_and_bare_kinds_still_say_something_plain() {
        assert_eq!(
            say(
                "mcp__linear__create_issue",
                json!({"title": "Crash on start"})
            ),
            ("use create issue (linear)".into(), "Crash on start".into())
        );
        assert_eq!(
            say("other", json!({"title": "Search X for posts"})),
            ("use a tool".into(), "Search X for posts".into())
        );
        assert_eq!(
            say("parzi__github_issue", json!({"title": "Bug"})),
            ("file a GitHub issue".into(), "Bug".into())
        );
    }

    #[test]
    fn a_write_outside_the_folder_says_so() {
        let a = ask_text(
            "Write",
            &json!({"file_path": "C:/elsewhere/x.txt", "outside": true}),
        );
        assert_eq!(a.action, "write x.txt");
        assert_eq!(a.note, "Outside this session's folder.");
    }

    #[test]
    fn only_media_makers_count_as_media() {
        for yes in [
            "image_gen",
            "image_edit",
            "reference_to_video",
            "text_to_video",
            "generate_image",
        ] {
            assert!(makes_media(yes), "{yes}");
        }
        for no in [
            "delete_image",
            "image_upload",
            "run_terminal_command",
            "video_delete",
        ] {
            assert!(!makes_media(no), "{no}");
        }
    }
}
