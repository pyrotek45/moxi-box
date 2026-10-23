use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use serde_json::{json, Value};

use crate::domain::{Settings, WorkspaceEntry};

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_SEARCH_RESULTS: usize = 50;
const MAX_TREE_ENTRIES: usize = 500;
const MAX_COMMAND_OUTPUT_BYTES: usize = 32 * 1024;

pub fn definitions() -> Vec<Value> {
    vec![
        definition("remember", "Save a concise durable project fact to workspace memory. Requires approval. Never store secrets.", json!({"type":"object","properties":{"note":{"type":"string"}},"required":["note"]})),
        definition(
            "list_files",
            "List the workspace files, limited to 50 entries.",
            json!({"type":"object","properties":{}}),
        ),
        definition(
            "read_file",
            "Read numbered lines from a UTF-8 workspace file. Use start_line and max_lines for larger files.",
            json!({"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"max_lines":{"type":"integer","minimum":1,"maximum":500}},"required":["path"]}),
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
    matches!(name, "write_file" | "run_command" | "remember")
}

pub fn approval_summary(name: &str, arguments: &Value) -> String {
    match name {
        "remember" => format!(
            "Remember project note: {}",
            arguments["note"].as_str().unwrap_or("")
        ),
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

pub fn execute(
    settings: &Settings,
    name: &str,
    arguments: &Value,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<String, String> {
    let root = canonical_root(&settings.workspace_root)?;
    match name {
        "list_files" => list_files(&root),
        "read_file" => {
            let path = required_string(arguments, "path")?;
            let start = arguments["start_line"]
                .as_u64()
                .unwrap_or(1)
                .clamp(1, usize::MAX as u64) as usize;
            let count = arguments["max_lines"].as_u64().unwrap_or(200).clamp(1, 500) as usize;
            let text = read_file(&root, path)?;
            let lines = text.lines().collect::<Vec<_>>();
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
        "exit status: {}\n{}{}",
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

pub fn list_folders(path: &str) -> Result<(String, Vec<String>), String> {
    let root = canonical_root(path)?;
    if !root.is_dir() {
        return Err("Choose a directory.".to_owned());
    }
    let mut folders = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().is_dir() {
            folders.push(entry.path().to_string_lossy().into_owned());
        }
    }
    folders.sort_by_key(|path| path.to_lowercase());
    Ok((root.to_string_lossy().into_owned(), folders))
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
        assert!(write_file(&fixture.0, "directory/new/file", "bad").is_err());
        assert!(read_file(&fixture.0, "link").is_err());
        assert!(workspace_entries(fixture.0.to_str().unwrap())
            .unwrap()
            .is_empty());
        assert!(search_content(&fixture.0, "private").unwrap().is_empty());
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
    fn workspace_browser_creation_and_reset_preserve_original_files() {
        let fixture = Fixture::new();
        let project = fixture.0.join("project");
        let (root, _) = open_workspace(project.to_str().unwrap(), true).unwrap();
        fs::write(project.join("important.txt"), "keep me").unwrap();
        assert!(open_workspace(project.to_str().unwrap(), true).is_err());
        assert!(list_folders(fixture.0.to_str().unwrap())
            .unwrap()
            .1
            .contains(&root));
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
