//! Reusable Python scripts the agent saves after they work ("skills"), shared by every
//! workspace. Each skill is one file in Moxi's data folder, so people can read, edit, or back
//! them up directly. Saving and running both need approval: skills are code that runs with the
//! user's permissions.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::{json, Value};

use crate::domain::Skill;

const DESCRIPTION_MARK: &str = "# moxi-skill:";
const USAGE_MARK: &str = "# usage:";
const MAX_CODE_BYTES: usize = 64 * 1024;
/// At most this many skills are listed to the model, to keep the context small.
const MAX_LISTED: usize = 40;

/// The skills folder: `skills` beside the database, or MOXI_SKILLS_DIR when set.
pub fn directory() -> Result<PathBuf, String> {
    if let Some(directory) = std::env::var_os("MOXI_SKILLS_DIR") {
        return Ok(PathBuf::from(directory));
    }
    // Tests never read or write the user's real skills.
    if cfg!(test) {
        // One folder per test thread, so parallel tests cannot disturb each other.
        let thread = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>();
        return Ok(
            std::env::temp_dir().join(format!("moxi-test-skills-{}-{thread}", std::process::id()))
        );
    }
    Ok(crate::storage::data_directory()?.join("skills"))
}

pub fn definitions() -> Vec<Value> {
    vec![
        crate::tools::definition(
            "save_skill",
            "Save a reusable Python script as a skill for later runs in any workspace, after you have run equivalent code successfully in this run. Good skills do one useful job, take inputs as command-line arguments, print clear results, and use only the standard library. Requires approval; the user sees the code.",
            json!({"type":"object","properties":{
                "name":{"type":"string","description":"Lowercase name with underscores, for example count_todos"},
                "description":{"type":"string","description":"One line: what it does"},
                "usage":{"type":"string","description":"Arguments, for example: <folder> [--ext .py]"},
                "code":{"type":"string","description":"Complete Python 3 script"}
            },"required":["name","description","code"]}),
        ),
        crate::tools::definition(
            "run_skill",
            "Run a saved skill with arguments from the workspace folder. Uses the project's .venv Python when present. Requires approval.",
            json!({"type":"object","properties":{
                "name":{"type":"string"},
                "args":{"type":"array","items":{"type":"string"},"description":"Command-line arguments"}
            },"required":["name"]}),
        ),
    ]
}

pub fn list() -> Result<Vec<Skill>, String> {
    list_in(&directory()?)
}

pub fn list_in(directory: &Path) -> Result<Vec<Skill>, String> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(Vec::new());
    };
    let mut skills = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_str()?.to_owned();
            (path.extension()? == "py" && valid_name(&name).is_ok()).then_some((name, path))
        })
        .filter_map(|(name, path)| {
            let code = fs::read_to_string(path).ok()?;
            let header = |mark: &str| {
                code.lines()
                    .take(5)
                    .find_map(|line| line.strip_prefix(mark))
                    .map(|text| text.trim().to_owned())
                    .unwrap_or_default()
            };
            Some(Skill {
                description: header(DESCRIPTION_MARK),
                usage: header(USAGE_MARK),
                name,
                code,
            })
        })
        .collect::<Vec<_>>();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

pub fn check_name(name: &str) -> Result<(), String> {
    valid_name(name)
}

fn valid_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 40
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err("Use a skill name of lowercase letters, digits, and underscores (at most 40), starting with a letter, for example count_todos.".to_owned())
    }
}

/// Validate and save; the file is checked for syntax errors before it replaces anything.
pub fn save_in(
    directory: &Path,
    name: &str,
    description: &str,
    usage: &str,
    code: &str,
) -> Result<PathBuf, String> {
    valid_name(name)?;
    let description = description.trim();
    if description.is_empty() || description.len() > 200 || description.contains('\n') {
        return Err("Give a one-line description under 200 characters.".to_owned());
    }
    let usage = usage.trim().replace('\n', " ");
    if usage.len() > 300 {
        return Err("Keep usage under 300 characters.".to_owned());
    }
    let body = code
        .lines()
        .filter(|line| !line.starts_with(DESCRIPTION_MARK) && !line.starts_with(USAGE_MARK))
        .collect::<Vec<_>>()
        .join("\n");
    if body.trim().is_empty() || body.len() > MAX_CODE_BYTES {
        return Err("Give the complete script, under 64 KiB.".to_owned());
    }
    // Keep a shebang first so the file still runs directly.
    let (shebang, rest) = match body.strip_prefix("#!") {
        Some(_) => body.split_once('\n').unwrap_or((&body, "")),
        None => ("", body.as_str()),
    };
    let mut text = String::new();
    if !shebang.is_empty() {
        text.push_str(shebang);
        text.push('\n');
    }
    text.push_str(&format!("{DESCRIPTION_MARK} {description}\n"));
    if !usage.is_empty() {
        text.push_str(&format!("{USAGE_MARK} {usage}\n"));
    }
    text.push_str(rest.trim_start_matches('\n'));
    if !text.ends_with('\n') {
        text.push('\n');
    }
    fs::create_dir_all(directory)
        .map_err(|error| format!("Cannot create the skills folder: {error}"))?;
    let draft = directory.join(format!(".{name}.draft.py"));
    fs::write(&draft, &text).map_err(|error| error.to_string())?;
    if let Some(problem) = crate::tools::syntax_problem(&draft) {
        let _ = fs::remove_file(&draft);
        return Err(format!(
            "The skill was not saved because it has a syntax error: {problem}"
        ));
    }
    let path = directory.join(format!("{name}.py"));
    fs::rename(&draft, &path).map_err(|error| error.to_string())?;
    Ok(path)
}

pub fn delete(name: &str) -> Result<(), String> {
    valid_name(name)?;
    let path = directory()?.join(format!("{name}.py"));
    fs::remove_file(&path).map_err(|error| format!("Cannot delete skill {name}: {error}"))
}

/// The script for a saved skill, or a helpful error listing what exists.
pub fn path_in(directory: &Path, name: &str) -> Result<PathBuf, String> {
    valid_name(name)?;
    let path = directory.join(format!("{name}.py"));
    if path.is_file() {
        return Ok(path);
    }
    let names = list_in(directory)?
        .into_iter()
        .map(|skill| skill.name)
        .collect::<Vec<_>>();
    Err(if names.is_empty() {
        format!("There is no skill named {name}; no skills are saved yet.")
    } else {
        format!(
            "There is no skill named {name}. Saved skills: {}.",
            names.join(", ")
        )
    })
}

/// Arguments as a list of strings; a single string is split on whitespace.
pub fn arguments(value: &Value) -> Result<Vec<String>, String> {
    match value.get("args").or_else(|| value.get("arguments")) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(text)) => Ok(text.split_whitespace().map(str::to_owned).collect()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => Ok(text.clone()),
                Value::Number(number) => Ok(number.to_string()),
                _ => Err("Each argument must be a string.".to_owned()),
            })
            .collect(),
        Some(_) => Err("Give args as a list of strings.".to_owned()),
    }
}

/// The Python to run skills with: the workspace's virtual environment when it has one.
pub fn python(root: &Path) -> PathBuf {
    for candidate in [
        ".venv/bin/python",
        "venv/bin/python",
        ".venv/Scripts/python.exe",
    ] {
        let path = root.join(candidate);
        if path.is_file() {
            return path;
        }
    }
    PathBuf::from("python3")
}

/// The system-message list of saved skills.
pub fn context_note(skills: &[Skill]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut lines = skills
        .iter()
        .take(MAX_LISTED)
        .map(|skill| {
            let usage = if skill.usage.is_empty() {
                String::new()
            } else {
                format!(" (usage: {})", skill.usage)
            };
            format!("- {}: {}{usage}", skill.name, skill.description)
        })
        .collect::<Vec<_>>();
    if skills.len() > MAX_LISTED {
        lines.push(format!("- ...and {} more", skills.len() - MAX_LISTED));
    }
    Some(format!(
        "Saved skills (reusable Python scripts; run one with run_skill in Agent mode instead of rewriting it):\n{}",
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_save_list_and_reject_bad_input() {
        let dir = std::env::temp_dir().join(format!("moxi-skills-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(list_in(&dir).unwrap().is_empty());
        let path = save_in(
            &dir,
            "count_lines",
            "Count lines in a file",
            "<file>",
            "#!/usr/bin/env python3\nimport sys\nprint(sum(1 for _ in open(sys.argv[1])))\n",
        )
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("#!/usr/bin/env python3\n# moxi-skill: Count lines in a file\n# usage: <file>\nimport sys"));
        let skills = list_in(&dir).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].description, "Count lines in a file");
        assert_eq!(skills[0].usage, "<file>");
        // Saving again replaces the header instead of stacking another one.
        save_in(&dir, "count_lines", "Count lines", "", &skills[0].code).unwrap();
        assert_eq!(
            fs::read_to_string(&path)
                .unwrap()
                .matches(DESCRIPTION_MARK)
                .count(),
            1
        );
        assert!(save_in(&dir, "Bad-Name", "x", "", "print(1)").is_err());
        assert!(save_in(&dir, "ok", "", "", "print(1)").is_err());
        let broken = save_in(&dir, "broken", "Broken", "", "def f(:\n  pass\n");
        assert!(broken.unwrap_err().contains("syntax error"));
        assert!(!dir.join("broken.py").exists() && !dir.join(".broken.draft.py").exists());
        assert!(path_in(&dir, "missing")
            .unwrap_err()
            .contains("count_lines"));
        assert_eq!(arguments(&json!({"args": "a b"})).unwrap(), ["a", "b"]);
        assert_eq!(
            arguments(&json!({"args": ["x y", 3]})).unwrap(),
            ["x y", "3"]
        );
        assert!(context_note(&list_in(&dir).unwrap())
            .unwrap()
            .contains("- count_lines: Count lines"));
        let _ = fs::remove_dir_all(dir);
    }
}
