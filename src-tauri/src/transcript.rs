// What the chat shows of Claude Code's transcript: each record the follower
// passes on is reduced to items here. Pure, so it can be run on a recorded
// transcript without a VM.
use serde_json::{json, Value};

// How much of a tool's output and of its one-line summary the chat keeps.
pub struct Limits { pub output_chars: usize, pub summary_chars: usize }

fn clipped(text: &str, limit: usize) -> String {
    let mut characters = text.chars();
    let head: String = characters.by_ref().take(limit).collect();
    if characters.next().is_some() { head + "…" } else { head }
}

fn between<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = text.find(&open)? + open.len();
    Some(&text[start..start + text[start..].find(&format!("</{tag}>"))?])
}

// What a command printed, without the terminal's colours.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' { out.push(character); }
        else if characters.next() == Some('[') { for code in characters.by_ref() { if code.is_ascii_alphabetic() { break; } } }
    }
    out
}

fn line_count(value: &Value) -> usize {
    value.as_str().filter(|text| !text.is_empty()).map(|text| text.split('\n').count()).unwrap_or(0)
}

fn tool_summary(name: &str, input: &Value, limit: usize) -> String {
    let field = match name {
        "Bash" => "command",
        "Read" | "Write" | "Edit" | "MultiEdit" => "file_path",
        "NotebookEdit" => "notebook_path",
        "Grep" | "Glob" => "pattern",
        "WebFetch" => "url",
        "WebSearch" => "query",
        "Task" | "Agent" => "description",
        "Skill" => "skill",
        _ => "",
    };
    let text = input.get(field).and_then(Value::as_str)
        .or_else(|| input["questions"][0]["question"].as_str())
        .or_else(|| input.as_object().and_then(|object| object.values().find_map(Value::as_str)))
        .unwrap_or("");
    clipped(text, limit)
}

// One tool call: what it acts on, its input as the chat shows it, and for an
// edit how many lines go in and out.
fn tool_item(id: String, at: &Value, block: &Value, limits: &Limits) -> Value {
    let name = block["name"].as_str().unwrap_or_default();
    let input = &block["input"];
    let edits = match &input["edits"] { Value::Array(edits) => edits.clone(), _ => vec![input.clone()] };
    let added: usize = edits.iter().map(|edit| ["new_string", "content", "new_source"].iter().map(|field| line_count(&edit[*field])).sum::<usize>()).sum();
    let removed: usize = edits.iter().map(|edit| line_count(&edit["old_string"])).sum();
    let shown = match input["command"].as_str() { Some(command) => command.to_owned(), None => serde_json::to_string_pretty(input).unwrap() };
    json!({"kind":"tool","id":id,"at":at,"tool_id":block["id"],"name":name,
        "summary":tool_summary(name, input, limits.summary_chars),"detail":input["description"],
        "input":clipped(&shown, limits.output_chars),"added":added,"removed":removed,"questions":input["questions"]})
}

fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|block| block["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

// Text the CLI wrote in the user's place: an interruption, a command of its
// own, or what such a command printed. None for the user's own words.
fn recorded(id: &str, at: &Value, text: &str, limits: &Limits) -> Option<Value> {
    if text.starts_with("[Request interrupted by user") { return Some(json!({"kind":"interrupt","id":id,"at":at})); }
    if let Some(name) = between(text, "command-name") {
        let arguments = between(text, "command-args").unwrap_or_default();
        return Some(json!({"kind":"command","id":id,"at":at,"text":format!("{name} {arguments}").trim()}));
    }
    if let Some(command) = between(text, "bash-input") { return Some(json!({"kind":"command","id":id,"at":at,"text":format!("! {command}")})); }
    let output = plain(between(text, "local-command-stdout").or_else(|| between(text, "bash-stdout"))?);
    Some(json!({"kind":"notice","id":id,"at":at,"text":clipped(output.trim(), limits.output_chars)}))
}

// What the user's side of the conversation holds: their words, the results
// of the tools Claude called, and what the CLI wrote in their place. Text
// the CLI did not mark as a prompt (`prompted`) and that opens with a tag is
// a note of its own to Claude, not something the user said.
fn said(id: &str, at: &Value, content: &Value, prompted: bool, limits: &Limits) -> Vec<Value> {
    let blocks = match content { Value::String(text) => vec![json!({"type":"text","text":text})], Value::Array(blocks) => blocks.clone(), _ => vec![] };
    let images = blocks.iter().filter(|block| block["type"] == "image").count();
    let mut items = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let key = format!("{id}:{index}");
        match block["type"].as_str() {
            Some("text") => {
                let text = block["text"].as_str().unwrap_or_default();
                match recorded(&key, at, text, limits) {
                    Some(item) => items.push(item),
                    None if prompted || !text.starts_with('<') => items.push(json!({"kind":"user","id":key,"at":at,"text":text,"images":images})),
                    None => (),
                }
            },
            Some("tool_result") => items.push(json!({"kind":"tool_result","id":key,"at":at,"tool_id":block["tool_use_id"],
                "output":clipped(&result_text(&block["content"]), limits.output_chars),"error":block["is_error"] == true})),
            _ => (),
        }
    }
    items
}

// Reduce one transcript record to what the chat shows.
pub fn chat_items(record: &Value, limits: &Limits) -> Vec<Value> {
    let id = record["uuid"].as_str().unwrap_or_default();
    let at = &record["timestamp"];
    let mut items = Vec::new();
    match record["type"].as_str() {
        Some("ai-title") => items.push(json!({"kind":"title","text":record["aiTitle"]})),
        Some("queue-operation") => items.push(json!({"kind":"queue","op":record["operation"],"text":record["content"]})),
        // The summary a compaction leaves is for Claude; its boundary is the mark the chat shows.
        Some("user") if record["isMeta"] != true && record["isCompactSummary"] != true => items.extend(said(id, at, &record["message"]["content"], record["promptSource"].is_string(), limits)),
        // A message queued while Claude worked reaches it between two steps of the same turn.
        Some("attachment") if record["attachment"]["type"] == "queued_command" => items.extend(said(id, at, &record["attachment"]["prompt"], true, limits)),
        Some("assistant") => {
            for (index, block) in record["message"]["content"].as_array().into_iter().flatten().enumerate() {
                let key = format!("{id}:{index}");
                match block["type"].as_str() {
                    Some("text") => items.push(json!({"kind":"assistant","id":key,"at":at,"text":block["text"]})),
                    Some("thinking") => items.push(json!({"kind":"thinking","id":key,"at":at,"text":block["thinking"],"ms":record["thinkingDurationMs"]})),
                    Some("tool_use") => items.push(tool_item(key, at, block, limits)),
                    _ => (),
                }
            }
        },
        Some("system") => match record["subtype"].as_str() {
            Some("turn_duration") => items.push(json!({"kind":"turn_end","id":id,"at":at,"ms":record["durationMs"]})),
            Some("compact_boundary") => items.push(json!({"kind":"compact","id":id,"at":at,
                "trigger":record["compactMetadata"]["trigger"],"tokens":record["compactMetadata"]["preTokens"]})),
            Some("local_command") => items.extend(recorded(id, at, record["content"].as_str().unwrap_or_default(), limits)),
            Some("informational") | Some("away_summary") => items.push(json!({"kind":"notice","id":id,"at":at,"text":record["content"]})),
            _ => (),
        },
        _ => (),
    }
    items.retain(|item| item["kind"] != "notice" || item["text"].as_str().is_some_and(|text| !text.is_empty()));
    items
}
