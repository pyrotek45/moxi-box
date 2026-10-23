use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use serde_json::{json, Value};

use crate::core::{Settings, WorkspaceEntry};

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_SEARCH_RESULTS: usize = 50;
const MAX_TREE_ENTRIES: usize = 500;
const MAX_COMMAND_OUTPUT_BYTES: usize = 32 * 1024;

pub fn definitions() -> Vec<Value> {
    vec![
        definition(
            "list_files",
            "List the workspace files, limited to 50 entries.",
            json!({"type":"object","properties":{}}),
        ),
        definition(
            "read_file",
            "Read a UTF-8 file in the workspace.",
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        ),
        definition(
            "search_content",
            "Find text in workspace files.",
            json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        ),
        definition(
            "write_file",
            "Create or replace a UTF-8 workspace file. Requires approval.",
            json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
        ),
        definition(
            "run_command",
            "Run an executable with arguments in the workspace. Requires approval.",
            json!({"type":"object","properties":{"program":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}},"required":["program"]}),
        ),
    ]
}

pub fn requires_approval(name: &str) -> bool {
    matches!(name, "write_file" | "run_command")
}

pub fn approval_summary(name: &str, arguments: &Value) -> String {
    match name {
        "write_file" => format!(
            "Write {}",
            arguments["path"].as_str().unwrap_or("a workspace file")
        ),
        "run_command" => format!(
            "Run {} {}",
            arguments["program"].as_str().unwrap_or("command"),
            arguments["args"]
                .as_array()
                .map(|args| args
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "))
                .unwrap_or_default()
        ),
        _ => format!("Use {name}"),
    }
}

pub fn execute(settings: &Settings, name: &str, arguments: &Value) -> Result<String, String> {
    let root = canonical_root(&settings.workspace_root)?;
    match name {
        "list_files" => list_files(&root),
        "read_file" => read_file(&root, required_string(arguments, "path")?),
        "search_content" => search_content(&root, required_string(arguments, "query")?),
        "write_file" => write_file(
            &root,
            required_string(arguments, "path")?,
            required_string(arguments, "content")?,
        ),
        "run_command" => run_command(
            &root,
            required_string(arguments, "program")?,
            arguments["args"].as_array(),
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

fn definition(name: &str, description: &str, parameters: Value) -> Value {
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
    let checked = if may_not_exist {
        fs::canonicalize(candidate.parent().unwrap_or(root))
            .map_err(|error| error.to_string())?
            .join(
                candidate
                    .file_name()
                    .ok_or_else(|| "Invalid file path.".to_owned())?,
            )
    } else {
        fs::canonicalize(&candidate).map_err(|error| error.to_string())?
    };
    if !checked.starts_with(root) {
        return Err("Path escapes the selected workspace.".to_owned());
    }
    Ok(checked)
}
fn list_files(root: &Path) -> Result<String, String> {
    let mut entries = fs::read_dir(root)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    entries.sort();
    entries.truncate(MAX_SEARCH_RESULTS);
    Ok(entries.join("\n"))
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
fn search_content(root: &Path, query: &str) -> Result<String, String> {
    let mut found = Vec::new();
    search_directory(root, root, query, &mut found)?;
    Ok(found.join("\n"))
}
fn search_directory(
    root: &Path,
    directory: &Path,
    query: &str,
    found: &mut Vec<String>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
    {
        if found.len() >= MAX_SEARCH_RESULTS {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            if !path
                .file_name()
                .is_some_and(|name| name == ".git" || name == "target")
            {
                search_directory(root, &path, query, found)?;
            }
        } else if entry.metadata().map_err(|error| error.to_string())?.len() <= MAX_FILE_BYTES {
            if let Ok(content) = fs::read_to_string(&path) {
                for (line, text) in content.lines().enumerate() {
                    if text.contains(query) {
                        found.push(format!(
                            "{}:{}: {}",
                            path.strip_prefix(root).unwrap_or(&path).display(),
                            line + 1,
                            text
                        ));
                        if found.len() >= MAX_SEARCH_RESULTS {
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(())
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
fn run_command(
    root: &Path,
    program: &str,
    arguments: Option<&Vec<Value>>,
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
    let output = Command::new(program)
        .args(&args)
        .current_dir(root)
        .output()
        .map_err(|error| format!("Could not run command: {error}"))?;
    let mut text = format!(
        "exit status: {}\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.truncate(MAX_COMMAND_OUTPUT_BYTES);
    Ok(text)
}
