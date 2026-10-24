use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use serde_json::{json, Value};

use crate::domain::{Settings, WorkspaceEntry};

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_SEARCH_RESULTS: usize = 50;
const MAX_LISTED_FILES: usize = 300;
const MAX_TREE_ENTRIES: usize = 500;
const MAX_COMMAND_OUTPUT_BYTES: usize = 32 * 1024;

pub fn definitions() -> Vec<Value> {
    vec![
        definition("replace_all", "Replace every literal occurrence of old_text in one workspace file. Requires approval. Read the file first; no regex. Fails without changes when no match exists.", json!({"type":"object","properties":{"path":{"type":"string"},"old_text":{"type":"string","minLength":1},"new_text":{"type":"string"}},"required":["path","old_text","new_text"]})),
        definition("list_tasks", "Read the saved workspace task list. Use at the start of multi-step work and when resuming.", json!({"type":"object","properties":{}})),
        definition("update_tasks", "Replace the entire workspace task list atomically, like TodoWrite. Include all items you want to keep. Create a short plan for multi-step work; update status as work progresses. One call can complete many tasks. Mark completed only after verifying success; failed or unfinished work stays pending or in_progress. Use [] only to intentionally clear the list. No approval needed; this changes task metadata only.", json!({"type":"object","additionalProperties":false,"properties":{"tasks":{"type":"array","maxItems":100,"items":{"type":"object","additionalProperties":false,"properties":{"title":{"type":"string","minLength":1,"maxLength":200},"status":{"type":"string","enum":["pending","in_progress","completed"]}},"required":["title","status"]}}},"required":["tasks"]})),
        definition("remember", "Save a concise durable project fact to workspace memory. Requires approval. Never store secrets.", json!({"type":"object","properties":{"note":{"type":"string"}},"required":["note"]})),
        definition(
            "list_files",
            "List files and folders recursively (folders end with /), skipping .git, target, and node_modules. Optional path lists a subfolder.",
            json!({"type":"object","properties":{"path":{"type":"string","description":"Relative folder; omit for the workspace root."}}}),
        ),
        definition(
            "read_file",
            "Read numbered lines from a UTF-8 workspace file. Use start_line and max_lines for larger files.",
            json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"max_lines":{"type":"integer","minimum":1,"maximum":500}},"required":["path"]}),
        ),
        definition(
            "search_content",
            "Find lines containing text in workspace files. Returns path:line: text, up to 50 matches. Optional path limits the search to a folder or file.",
            json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"},"ignore_case":{"type":"boolean"}},"required":["query"]}),
        ),
        definition(
            "write_file",
            "Create a UTF-8 workspace file, or replace a whole file. Parent folders are created. Requires approval.",
            json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
        ),
        definition(
            "edit_file",
            "Replace one exact occurrence of old_text with new_text in an existing workspace file. Read the file first and copy old_text exactly, including whitespace but without read_file's line-number prefixes. Include enough lines to make it unique. Prefer this over write_file for changes to existing files. Requires approval.",
            json!({"type":"object","properties":{"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"}},"required":["path","old_text","new_text"]}),
        ),
        definition(
            "run_command",
            "Run a program with arguments from the workspace root, e.g. program cargo, args [\"test\"]. For pipes, && or redirects use program sh with args [\"-c\", \"...\"]. Use it to build, test, use git, or move and delete files. Output is capped. Requires approval.",
            json!({"type":"object","properties":{"program":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}},"required":["program"]}),
        ),
    ]
}

pub fn requires_approval(name: &str) -> bool {
    matches!(
        name,
        "write_file" | "replace_all" | "edit_file" | "run_command" | "remember"
    )
}

/// A short, readable name for a call: "Edit src/a.rs" while pending, "Edited src/a.rs" when done.
pub fn label(name: &str, arguments: &Value, done: bool) -> String {
    let verb = |pending: &str, past: &str| if done { past } else { pending }.to_owned();
    let path = arguments["path"].as_str().filter(|path| !path.is_empty());
    let target = |fallback: &str| path.unwrap_or(fallback).to_owned();
    match name {
        "list_files" => match path {
            Some(path) => format!("{} in {path}", verb("List files", "Listed files")),
            None => verb("List files", "Listed files"),
        },
        "read_file" => format!("{} {}", verb("Read", "Read"), target("a file")),
        "search_content" => format!(
            "{} \"{}\"",
            verb("Search for", "Searched for"),
            arguments["query"].as_str().unwrap_or("")
        ),
        "write_file" => format!("{} {}", verb("Write", "Wrote"), target("a file")),
        "edit_file" => format!("{} {}", verb("Edit", "Edited"), target("a file")),
        "run_command" => format!("{} {}", verb("Run", "Ran"), command_line(arguments)),
        "list_tasks" => verb("Check the task list", "Checked the task list"),
        "update_tasks" => verb("Update the task list", "Updated the task list"),
        "remember" => verb("Save a project note", "Saved a project note"),
        "web_search" => format!(
            "{} \"{}\"",
            verb("Search the web for", "Searched the web for"),
            arguments["query"].as_str().unwrap_or("")
        ),
        "fetch_url" => format!(
            "{} {}",
            verb("Open", "Read"),
            arguments["url"].as_str().unwrap_or("a page")
        ),
        _ => format!("{} {name}", verb("Use", "Used")),
    }
}

/// What a mutating call will do, shown for approval and kept in the step afterwards.
pub fn call_preview(name: &str, arguments: &Value) -> String {
    let text = |key: &str| arguments[key].as_str().unwrap_or("");
    match name {
        "edit_file" => text("old_text")
            .lines()
            .map(|line| format!("- {line}"))
            .chain(text("new_text").lines().map(|line| format!("+ {line}")))
            .collect::<Vec<_>>()
            .join("\n"),
        "write_file" => text("content")
            .lines()
            .map(|line| format!("+ {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        "run_command" => format!("$ {}", command_line(arguments)),
        "remember" => text("note").to_owned(),
        _ => String::new(),
    }
}

/// Step details for people: the useful result, not raw JSON arguments.
pub fn result_detail(name: &str, arguments: &Value, result: &str) -> String {
    let preview = call_preview(name, arguments);
    match name {
        "update_tasks" | "list_tasks" => {
            let tasks =
                serde_json::from_str::<Vec<crate::domain::Task>>(result).unwrap_or_default();
            if tasks.is_empty() {
                return "No tasks.".to_owned();
            }
            tasks
                .iter()
                .map(|task| {
                    let mark = match task.status {
                        crate::domain::TaskStatus::Completed => "[x]",
                        crate::domain::TaskStatus::InProgress => "[~]",
                        crate::domain::TaskStatus::Pending => "[ ]",
                    };
                    format!("{mark} {}", task.title)
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        "run_command" => format!("{preview}\n{result}"),
        _ if preview.is_empty() => result.to_owned(),
        _ => format!("{result}\n\n{preview}"),
    }
}

fn command_line(arguments: &Value) -> String {
    let args = arguments["args"]
        .as_array()
        .map(|args| {
            args.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    format!(
        "{} {args}",
        arguments["program"].as_str().unwrap_or("command")
    )
    .trim()
    .to_owned()
}

pub fn approval_summary(name: &str, arguments: &Value) -> String {
    match name {
        "remember" => "Remember this project note?".to_owned(),
        "write_file" => format!(
            "Write {} ({} bytes)?",
            arguments["path"].as_str().unwrap_or("a workspace file"),
            arguments["content"].as_str().map_or(0, str::len)
        ),
        "edit_file" => format!(
            "Edit {}?",
            arguments["path"].as_str().unwrap_or("a workspace file")
        ),
        "run_command" => "Run this command in the workspace?".to_owned(),
        _ => format!("Use {name}?"),
    }
}

pub fn execute(
    settings: &Settings,
    name: &str,
    arguments: &Value,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<String, String> {
    let root = canonical_root(&settings.workspace_root)?;
    match name {
        "list_files" => list_files(&root, arguments["path"].as_str().unwrap_or("")),
        "read_file" => {
            let path = required_string(arguments, "path")?;
            let start = arguments["start_line"]
                .as_u64()
                .unwrap_or(1)
                .clamp(1, usize::MAX as u64) as usize;
            let count = arguments["max_lines"].as_u64().unwrap_or(200).clamp(1, 500) as usize;
            let text = read_file(&root, path)?;
            let lines = text.lines().collect::<Vec<_>>();
            if start > lines.len().max(1) {
                return Err(format!("{path} has only {} lines.", lines.len()));
            }
            let mut output = lines
                .iter()
                .enumerate()
                .skip(start - 1)
                .take(count)
                .map(|(index, line)| format!("{}: {}", index + 1, line))
                .collect::<Vec<_>>()
                .join("\n");
            if start.saturating_add(count) <= lines.len() {
                output.push_str("\n[More lines remain. Request the next range.]");
            }
            Ok(output)
        }
        "search_content" => {
            let scope = match arguments["path"].as_str().map(str::trim) {
                Some(path) if !path.is_empty() && path != "." => safe_path(&root, path, false)?,
                _ => root.clone(),
            };
            search_content(
                &root,
                &scope,
                required_string(arguments, "query")?,
                arguments["ignore_case"].as_bool().unwrap_or(false),
            )
        }
        "write_file" => write_file(
            &root,
            required_string(arguments, "path")?,
            required_string(arguments, "content")?,
        ),
        "replace_all" => replace_all(
            &root,
            required_string(arguments, "path")?,
            required_string(arguments, "old_text")?,
            required_string(arguments, "new_text")?,
        ),
        "edit_file" => edit_file(
            &root,
            required_string(arguments, "path")?,
            required_string(arguments, "old_text")?,
            required_string(arguments, "new_text")?,
        ),
        "run_command" => run_command(
            &root,
            required_string(arguments, "program")?,
            arguments["args"].as_array(),
            cancelled,
        ),
        _ => Err(format!("Unknown tool: {name}")),
    }
}

pub fn workspace_entries(root: &str) -> Result<Vec<WorkspaceEntry>, String> {
    let root = canonical_root(root)?;
    let mut entries = Vec::new();
    collect_entries(&root, &root, 0, &mut entries)?;
    Ok(entries)
}

pub fn preview(root: &str, path: &str) -> Result<String, String> {
    read_file(&canonical_root(root)?, path)
}

pub fn definition(name: &str, description: &str, parameters: Value) -> Value {
    json!({"type":"function","function":{"name":name,"description":description,"parameters":parameters}})
}
fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing string argument: {key}"))
}
fn canonical_root(root: &str) -> Result<PathBuf, String> {
    fs::canonicalize(root).map_err(|error| format!("Invalid workspace root: {error}"))
}
fn safe_path(root: &Path, input: &str, may_not_exist: bool) -> Result<PathBuf, String> {
    let relative = Path::new(input);
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("Path must be relative to the workspace.".to_owned());
    }
    let candidate = root.join(relative);
    let checked = if may_not_exist && fs::symlink_metadata(&candidate).is_err() {
        let mut ancestor = candidate.as_path();
        let mut missing = Vec::new();
        while fs::symlink_metadata(ancestor).is_err() {
            missing.push(
                ancestor
                    .file_name()
                    .ok_or_else(|| "Invalid file path.".to_owned())?,
            );
            ancestor = ancestor
                .parent()
                .ok_or_else(|| "Invalid file path.".to_owned())?;
        }
        let mut checked = fs::canonicalize(ancestor).map_err(|error| error.to_string())?;
        for component in missing.into_iter().rev() {
            checked.push(component);
        }
        checked
    } else {
        fs::canonicalize(&candidate).map_err(|error| error.to_string())?
    };
    if !checked.starts_with(root) {
        return Err("Path escapes the selected workspace.".to_owned());
    }
    Ok(checked)
}
fn list_files(root: &Path, path: &str) -> Result<String, String> {
    let directory = if path.trim().is_empty() || path.trim() == "." {
        root.to_path_buf()
    } else {
        safe_path(root, path.trim(), false)?
    };
    if !directory.is_dir() {
        return Err(format!("{path} is not a folder."));
    }
    let mut entries = Vec::new();
    collect_entries(root, &directory, 0, &mut entries)?;
    if entries.is_empty() {
        return Ok("(empty folder)".to_owned());
    }
    let mut listing = entries
        .iter()
        .take(MAX_LISTED_FILES)
        .map(|entry| {
            format!(
                "{}{}",
                entry.path,
                if entry.is_directory { "/" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if entries.len() > MAX_LISTED_FILES {
        listing.push_str("\n[More entries remain. List a subfolder.]");
    }
    Ok(listing)
}
fn collect_entries(
    root: &Path,
    directory: &Path,
    depth: usize,
    entries: &mut Vec<WorkspaceEntry>,
) -> Result<(), String> {
    let mut children = fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    children.sort_by_key(|entry| (!entry.path().is_dir(), entry.file_name()));
    for child in children {
        if entries.len() >= MAX_TREE_ENTRIES {
            break;
        }
        if child
            .file_type()
            .map_err(|error| error.to_string())?
            .is_symlink()
        {
            continue;
        }
        let path = child.path();
        let name = child.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), ".git" | "target" | "node_modules") {
            continue;
        }
        let is_directory = path.is_dir();
        entries.push(WorkspaceEntry {
            path: path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string(),
            name,
            depth,
            is_directory,
        });
        if is_directory {
            collect_entries(root, &path, depth + 1, entries)?;
        }
    }
    Ok(())
}
fn read_file(root: &Path, path: &str) -> Result<String, String> {
    let path = safe_path(root, path, false)?;
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err("File exceeds the read limit.".to_owned());
    }
    fs::read_to_string(path).map_err(|error| error.to_string())
}
fn search_content(
    root: &Path,
    scope: &Path,
    query: &str,
    ignore_case: bool,
) -> Result<String, String> {
    if query.is_empty() {
        return Err("Search query must not be empty.".to_owned());
    }
    let query = if ignore_case {
        query.to_lowercase()
    } else {
        query.to_owned()
    };
    let mut found = Vec::new();
    if scope.is_file() {
        search_file(root, scope, &query, ignore_case, &mut found);
    } else {
        search_directory(root, scope, &query, ignore_case, &mut found)?;
    }
    if found.is_empty() {
        return Ok(format!("No matches for {query:?}."));
    }
    Ok(found.join("\n"))
}
fn search_directory(
    root: &Path,
    directory: &Path,
    query: &str,
    ignore_case: bool,
    found: &mut Vec<String>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
    {
        if found.len() >= MAX_SEARCH_RESULTS {
            break;
        }
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_symlink()
        {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if !path
                .file_name()
                .is_some_and(|name| name == ".git" || name == "target" || name == "node_modules")
            {
                search_directory(root, &path, query, ignore_case, found)?;
            }
        } else if entry.metadata().map_err(|error| error.to_string())?.len() <= MAX_FILE_BYTES {
            search_file(root, &path, query, ignore_case, found);
        }
    }
    Ok(())
}
fn search_file(root: &Path, path: &Path, query: &str, ignore_case: bool, found: &mut Vec<String>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    for (line, text) in content.lines().enumerate() {
        let matched = if ignore_case {
            text.to_lowercase().contains(query)
        } else {
            text.contains(query)
        };
        if matched {
            found.push(format!(
                "{}:{}: {}",
                path.strip_prefix(root).unwrap_or(path).display(),
                line + 1,
                text.chars().take(300).collect::<String>()
            ));
            if found.len() >= MAX_SEARCH_RESULTS {
                break;
            }
        }
    }
}
fn write_file(root: &Path, path: &str, content: &str) -> Result<String, String> {
    let path = safe_path(root, path, true)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&path, content).map_err(|error| error.to_string())?;
    Ok(format!(
        "Wrote {} bytes to {}",
        content.len(),
        path.strip_prefix(root).unwrap_or(&path).display()
    ))
}
fn replace_all(root: &Path, path: &str, old: &str, new: &str) -> Result<String, String> {
    if old.is_empty() {
        return Err("old_text must not be empty.".into());
    }
    let text = read_file(root, path)?;
    let count = text.matches(old).count();
    if count == 0 {
        return Err("old_text was not found. Read the file and copy the exact text.".into());
    }
    let replaced = text.replace(old, new);
    if replaced.len() > MAX_FILE_BYTES as usize {
        return Err("Replacement exceeds the file size limit.".into());
    }
    fs::write(safe_path(root, path, false)?, replaced).map_err(|e| e.to_string())?;
    Ok(format!("Replaced {count} occurrences in {path}."))
}

fn edit_file(root: &Path, path: &str, old: &str, new: &str) -> Result<String, String> {
    if old.is_empty() {
        return Err("old_text must not be empty. Use write_file to create a file.".to_owned());
    }
    let text = read_file(root, path)?;
    // Models send \n line endings; match files saved with \r\n too.
    let (old, new) = if !text.contains(old) && text.contains("\r\n") {
        (old.replace('\n', "\r\n"), new.replace('\n', "\r\n"))
    } else {
        (old.to_owned(), new.to_owned())
    };
    match text.matches(&old).count() {
        0 => return Err("old_text was not found. Read the file again and copy the exact text, including indentation.".to_owned()),
        1 => {}
        count => return Err(format!("old_text matches {count} places. Include more surrounding lines so it is unique.")),
    }
    let line = text[..text.find(&old).unwrap_or(0)].lines().count() + 1;
    fs::write(safe_path(root, path, false)?, text.replacen(&old, &new, 1))
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "Edited {path} at line {line}: replaced {} lines with {} lines.",
        old.lines().count(),
        new.lines().count()
    ))
}
fn run_command(
    root: &Path,
    program: &str,
    arguments: Option<&Vec<Value>>,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<String, String> {
    let args = arguments
        .into_iter()
        .flatten()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| "Command args must be strings.".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut command = Command::new(program);
    command
        .args(&args)
        .current_dir(root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not run command: {error}"))?;
    let (tx, rx) = std::sync::mpsc::channel();
    for (index, mut pipe) in [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .enumerate()
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut captured = Vec::new();
            let mut buffer = [0u8; 4096];
            if let Some(pipe) = pipe.as_mut() {
                loop {
                    match pipe.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            let keep =
                                count.min(MAX_COMMAND_OUTPUT_BYTES.saturating_sub(captured.len()));
                            captured.extend_from_slice(&buffer[..keep]);
                        }
                    }
                }
            }
            let _ = tx.send((index, captured));
        });
    }
    drop(tx);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            stop_command(&mut child);
            let _ = child.wait();
            return Err("Command stopped by user.".to_owned());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let mut output = [Vec::new(), Vec::new()];
    let mut received = 0;
    while received < 2 {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            stop_command(&mut child);
            return Err("Command stopped by user.".to_owned());
        }
        match rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok((index, bytes)) => {
                output[index] = bytes;
                received += 1;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Err("Command output could not be read.".to_owned()),
        }
    }
    let mut text = format!(
        "{}\n{}{}",
        status,
        String::from_utf8_lossy(&output[0]),
        String::from_utf8_lossy(&output[1])
    );
    let mut end = text.len().min(MAX_COMMAND_OUTPUT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    Ok(text)
}

fn stop_command(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
}

pub fn open_workspace(path: &str, create: bool) -> Result<(String, Vec<WorkspaceEntry>), String> {
    let path = Path::new(path.trim());
    if !path.is_absolute() {
        return Err("Enter an absolute workspace path.".to_owned());
    }
    if create {
        fs::create_dir(path).map_err(|error| {
            format!(
                "Cannot create workspace: {error}. Choose a new folder inside an existing parent."
            )
        })?;
    }
    let root = canonical_root(path.to_str().ok_or("Workspace path must be UTF-8.")?)?;
    if !root.is_dir() {
        return Err("Workspace must be a directory.".to_owned());
    }
    let root = root.to_string_lossy().into_owned();
    let entries = workspace_entries(&root)?;
    Ok((root, entries))
}

pub fn reset_workspace(path: &str) -> Result<String, String> {
    let input = Path::new(path);
    if fs::symlink_metadata(input)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Cannot reset a symlink. Choose the actual project directory.".to_owned());
    }
    let root = canonical_root(path)?;
    let home = std::env::var_os("HOME").and_then(|home| fs::canonicalize(home).ok());
    if !root.is_dir()
        || root.parent().is_none()
        || root.components().count() < 3
        || home.as_ref() == Some(&root)
    {
        return Err(
            "Choose a dedicated project folder, not a home or system directory.".to_owned(),
        );
    }
    let name = root
        .file_name()
        .ok_or("Invalid workspace name.")?
        .to_string_lossy();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let backup = root.with_file_name(format!("{name}.backup-{stamp}"));
    if backup.exists() {
        return Err("Backup already exists. Try again.".to_owned());
    }
    fs::rename(&root, &backup).map_err(|e| format!("Cannot archive workspace: {e}"))?;
    if let Err(error) = fs::create_dir(&root) {
        return match fs::rename(&backup,&root) {
            Ok(()) => Err(format!("Could not recreate workspace; original restored: {error}")),
            Err(rollback) => Err(format!("Could not recreate workspace ({error}) or restore it ({rollback}). Original files remain at {}",backup.display())),
        };
    }
    Ok(backup.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "moxi-tools-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn writes_nested_files_but_rejects_traversal() {
        let fixture = Fixture::new();
        write_file(&fixture.0, "nested/deep/file.txt", "hello").unwrap();
        assert_eq!(
            read_file(&fixture.0, "nested/deep/file.txt").unwrap(),
            "hello"
        );
        assert!(write_file(&fixture.0, "../escape.txt", "bad").is_err());
        assert!(read_file(&fixture.0, "/etc/passwd").is_err());
    }

    #[test]
    fn agent_file_tools_list_recursively_and_report_bad_requests() {
        let fixture = Fixture::new();
        write_file(&fixture.0, "src/main.rs", "fn main() {}\n").unwrap();
        write_file(&fixture.0, "README.md", "hello").unwrap();
        fs::create_dir_all(fixture.0.join("target/debug")).unwrap();
        assert_eq!(
            list_files(&fixture.0, "").unwrap(),
            "src/\nsrc/main.rs\nREADME.md"
        );
        assert_eq!(list_files(&fixture.0, "src").unwrap(), "src/main.rs");
        assert!(list_files(&fixture.0, "README.md").is_err());
        assert!(list_files(&fixture.0, "../").is_err());
        assert!(search_content(&fixture.0, &fixture.0, "", false).is_err());
        assert_eq!(
            search_content(&fixture.0, &fixture.0, "fn main", false).unwrap(),
            "src/main.rs:1: fn main() {}"
        );
        let settings = Settings {
            workspace_root: fixture.0.to_string_lossy().into_owned(),
            ..Settings::default()
        };
        let none = std::sync::atomic::AtomicBool::new(false);
        let read = |args: Value| execute(&settings, "read_file", &args, &none);
        assert_eq!(read(json!({"path":"README.md"})).unwrap(), "1: hello");
        assert!(read(json!({"path":"README.md","start_line":5}))
            .unwrap_err()
            .contains("only 1 lines"));
        let search = |args: Value| execute(&settings, "search_content", &args, &none);
        assert_eq!(
            search(json!({"query":"FN MAIN","ignore_case":true,"path":"src"})).unwrap(),
            "src/main.rs:1: fn main() {}"
        );
        assert!(search(json!({"query":"hello","path":"src"}))
            .unwrap()
            .starts_with("No matches"));
        assert_eq!(
            search(json!({"query":"hello","path":"README.md"})).unwrap(),
            "README.md:1: hello"
        );
        assert!(search(json!({"query":"x","path":"../"})).is_err());
        assert!(execute(&settings, "delete_everything", &json!({}), &none).is_err());
    }

    #[test]
    fn step_labels_and_details_are_readable() {
        let edit = json!({"path":"src/a.rs","old_text":"a\nb","new_text":"c"});
        assert_eq!(label("edit_file", &edit, true), "Edited src/a.rs");
        assert_eq!(label("edit_file", &edit, false), "Edit src/a.rs");
        assert_eq!(call_preview("edit_file", &edit), "- a\n- b\n+ c");
        let command = json!({"program":"cargo","args":["test","--locked"]});
        assert_eq!(
            label("run_command", &command, true),
            "Ran cargo test --locked"
        );
        assert_eq!(
            result_detail("run_command", &command, "exit status: 0"),
            "$ cargo test --locked\nexit status: 0"
        );
        assert_eq!(label("list_files", &json!({}), true), "Listed files");
        assert_eq!(
            result_detail(
                "update_tasks",
                &json!({}),
                r#"[{"id":1,"title":"Plan","status":"completed"},{"id":2,"title":"Build","status":"in_progress"}]"#
            ),
            "[x] Plan\n[~] Build"
        );
        assert_eq!(result_detail("read_file", &json!({}), "1: hi"), "1: hi");
    }

    #[test]
    fn edit_file_replaces_exactly_one_match_in_nested_files() {
        let fixture = Fixture::new();
        write_file(
            &fixture.0,
            "src/app/lib.rs",
            "fn a() {}\nfn b() {}\nfn b() {}\n",
        )
        .unwrap();
        edit_file(
            &fixture.0,
            "src/app/lib.rs",
            "fn a() {}",
            "fn a() { run(); }",
        )
        .unwrap();
        assert_eq!(
            read_file(&fixture.0, "src/app/lib.rs").unwrap(),
            "fn a() { run(); }\nfn b() {}\nfn b() {}\n"
        );
        assert!(edit_file(&fixture.0, "src/app/lib.rs", "fn b() {}", "x")
            .unwrap_err()
            .contains("2 places"));
        assert!(edit_file(&fixture.0, "src/app/lib.rs", "missing", "x")
            .unwrap_err()
            .contains("not found"));
        assert!(edit_file(&fixture.0, "src/app/lib.rs", "", "x").is_err());
        assert!(edit_file(&fixture.0, "../outside.rs", "a", "b").is_err());
        write_file(&fixture.0, "win.txt", "one\r\ntwo\r\n").unwrap();
        edit_file(&fixture.0, "win.txt", "one\ntwo", "1\n2").unwrap();
        assert_eq!(read_file(&fixture.0, "win.txt").unwrap(), "1\r\n2\r\n");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape_or_recurse() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let outside = Fixture::new();
        fs::write(outside.0.join("secret"), "private").unwrap();
        symlink(outside.0.join("secret"), fixture.0.join("link")).unwrap();
        symlink(&outside.0, fixture.0.join("directory")).unwrap();
        symlink(&fixture.0, fixture.0.join("cycle")).unwrap();
        assert!(write_file(&fixture.0, "link", "bad").is_err());
        assert!(edit_file(&fixture.0, "link", "private", "bad").is_err());
        assert!(write_file(&fixture.0, "directory/new/file", "bad").is_err());
        assert!(read_file(&fixture.0, "link").is_err());
        assert!(workspace_entries(fixture.0.to_str().unwrap())
            .unwrap()
            .is_empty());
        assert!(search_content(&fixture.0, &fixture.0, "private", false)
            .unwrap()
            .starts_with("No matches"));
        assert!(list_files(&fixture.0, "directory").is_err());
        assert_eq!(
            fs::read_to_string(outside.0.join("secret")).unwrap(),
            "private"
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_output_truncates_on_a_character_boundary() {
        let fixture = Fixture::new();
        let args = vec![json!("-c"), json!("printf 'é%.0s' $(seq 1 20000)")];
        let result = run_command(
            &fixture.0,
            "bash",
            Some(&args),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        assert!(result.len() <= MAX_COMMAND_OUTPUT_BYTES);
        assert!(result.contains('é'));
    }
    #[test]
    fn workspace_creation_and_reset_preserve_original_files() {
        let fixture = Fixture::new();
        let project = fixture.0.join("project");
        let (root, _) = open_workspace(project.to_str().unwrap(), true).unwrap();
        fs::write(project.join("important.txt"), "keep me").unwrap();
        assert!(open_workspace(project.to_str().unwrap(), true).is_err());
        let backup = reset_workspace(&root).unwrap();
        assert_eq!(
            fs::read_to_string(Path::new(&backup).join("important.txt")).unwrap(),
            "keep me"
        );
        assert!(project.is_dir());
        assert!(fs::read_dir(&project).unwrap().next().is_none());
        assert!(reset_workspace("/").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn user_can_stop_a_long_running_command() {
        let fixture = Fixture::new();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = cancelled.clone();
        let trigger = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            signal.store(true, std::sync::atomic::Ordering::Release);
        });
        let result = run_command(
            &fixture.0,
            "sh",
            Some(&vec![json!("-c"), json!("while :; do sleep 1; done")]),
            &cancelled,
        );
        trigger.join().unwrap();
        assert!(result.unwrap_err().contains("stopped by user"));
    }
}

#[cfg(test)]
mod replacement_tests {
    use super::*;
    #[test]
    fn replaces_all_and_preserves_file_on_missing_match() {
        let root = std::env::temp_dir().join(format!("moxi-replace-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("test.txt"), "boat boat").unwrap();
        assert!(replace_all(&root, "test.txt", "boat", "ship")
            .unwrap()
            .contains("2 occurrences"));
        assert!(replace_all(&root, "test.txt", "missing", "x").is_err());
        assert!(replace_all(&root, "test.txt", "", "x").is_err());
        assert_eq!(
            fs::read_to_string(root.join("test.txt")).unwrap(),
            "ship ship"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
