//! Notes are plain Markdown files (`<name>.md`) in a single folder.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct Note {
    pub name: String,
    pub path: PathBuf,
    pub modified: SystemTime,
    pub content: String,
    content_lower: String,
    name_lower: String,
}

pub struct NoteStore {
    pub dir: PathBuf,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    /// Index into `NoteStore::notes`.
    pub index: usize,
    pub score: i64,
    /// A line of content that matched the query (if the match was in the body).
    pub snippet: Option<String>,
}

impl NoteStore {
    pub fn new(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        let mut s = Self { dir, notes: Vec::new() };
        s.refresh();
        s
    }

    /// Re-scan the folder. File contents are only re-read when a file's mtime changed.
    pub fn refresh(&mut self) {
        let _ = fs::create_dir_all(&self.dir);
        let mut cache: HashMap<PathBuf, Note> =
            self.notes.drain(..).map(|n| (n.path.clone(), n)).collect();
        let mut notes = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if !is_note_file(&path) {
                    continue;
                }
                let modified = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                if let Some(old) = cache.remove(&path) {
                    if old.modified == modified {
                        notes.push(old);
                        continue;
                    }
                }
                let content = fs::read_to_string(&path).unwrap_or_default();
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                notes.push(Note::new(name, path, modified, content));
            }
        }
        notes.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name_lower.cmp(&b.name_lower)));
        self.notes = notes;
    }

    pub fn find_exact(&self, name: &str) -> Option<usize> {
        let l = name.trim().to_lowercase();
        self.notes.iter().position(|n| n.name_lower == l)
    }

    pub fn path_for(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.md", sanitize_name(name)))
    }

    /// Create an empty note (or return the existing one's path).
    pub fn create(&mut self, name: &str) -> io::Result<PathBuf> {
        let path = self.path_for(name);
        if !path.exists() {
            fs::write(&path, "")?;
        }
        self.refresh();
        Ok(path)
    }

    pub fn save(&mut self, path: &Path, content: &str) -> io::Result<SystemTime> {
        // Write to a temp file then rename, so a crash never leaves a half-written note.
        let tmp = path.with_extension("md.tmp");
        fs::write(&tmp, content)?;
        fs::rename(&tmp, path).or_else(|_| fs::write(path, content))?;
        let modified = fs::metadata(path).and_then(|m| m.modified()).unwrap_or_else(|_| SystemTime::now());
        if let Some(n) = self.notes.iter_mut().find(|n| n.path == path) {
            *n = Note::new(n.name.clone(), n.path.clone(), modified, content.to_string());
        }
        Ok(modified)
    }

    pub fn rename(&mut self, from: &Path, new_name: &str) -> io::Result<PathBuf> {
        let to = self.path_for(new_name);
        if to != from {
            let same_file_case_change = to.to_string_lossy().to_lowercase() == from.to_string_lossy().to_lowercase();
            if to.exists() && !same_file_case_change {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "A note with that name already exists"));
            }
            fs::rename(from, &to)?;
        }
        self.refresh();
        Ok(to)
    }

    /// Moves the note to the Recycle Bin (falls back to a permanent delete if that fails).
    pub fn delete(&mut self, path: &Path) -> io::Result<()> {
        if trash::delete(path).is_err() {
            fs::remove_file(path)?;
        }
        self.refresh();
        Ok(())
    }

    /// Ranked search over names and content. An empty query lists every note, newest first.
    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return (0..self.notes.len()).map(|i| SearchHit { index: i, score: 0, snippet: None }).collect();
        }
        let terms: Vec<&str> = q.split_whitespace().collect();
        let mut hits = Vec::new();
        for (i, n) in self.notes.iter().enumerate() {
            let mut score: i64 = 0;
            if n.name_lower == q {
                score += 10_000;
            } else if n.name_lower.starts_with(&q) {
                score += 5_000;
            } else if n.name_lower.contains(&q) {
                score += 3_000;
            } else if let Some(f) = fuzzy_score(&n.name_lower, &q) {
                score += 1_000 + f;
            }
            // Every term must appear in the name or the body for a body-only match.
            let mut snippet = None;
            if score == 0 {
                let all = terms.iter().all(|t| n.name_lower.contains(t) || n.content_lower.contains(t));
                if all {
                    score += 100 + terms.iter().filter(|t| n.name_lower.contains(*t)).count() as i64 * 50;
                    snippet = matching_line(&n.content, terms[0]);
                }
            }
            if score > 0 {
                hits.push(SearchHit { index: i, score, snippet });
            }
        }
        // Stable sort keeps the newest-first order for equal scores.
        hits.sort_by(|a, b| b.score.cmp(&a.score));
        hits
    }
}

impl Note {
    fn new(name: String, path: PathBuf, modified: SystemTime, content: String) -> Self {
        Self {
            name_lower: name.to_lowercase(),
            content_lower: content.to_lowercase(),
            name,
            path,
            modified,
            content,
        }
    }

    /// First non-empty line of the note, for the results list.
    pub fn preview(&self) -> String {
        self.content.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| truncate(l, 90)).unwrap_or_default()
    }
}

fn is_note_file(p: &Path) -> bool {
    p.is_file()
        && p.extension().map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("txt")).unwrap_or(false)
}

/// Subsequence match ("mtg nts" ~ "meeting notes"); higher is better.
fn fuzzy_score(hay: &str, needle: &str) -> Option<i64> {
    let needle: Vec<char> = needle.chars().filter(|c| !c.is_whitespace()).collect();
    if needle.is_empty() {
        return None;
    }
    let mut ni = 0;
    let mut score = 0i64;
    let mut prev_match = false;
    for c in hay.chars() {
        if ni < needle.len() && c == needle[ni] {
            ni += 1;
            score += if prev_match { 10 } else { 3 };
            prev_match = true;
        } else {
            prev_match = false;
        }
    }
    (ni == needle.len()).then_some(score)
}

fn matching_line(content: &str, term: &str) -> Option<String> {
    content
        .lines()
        .find(|l| l.to_lowercase().contains(term))
        .map(|l| truncate(l.trim(), 90))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

/// Make a note name safe as a Windows file name.
pub fn sanitize_name(name: &str) -> String {
    let mut s: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    if s.is_empty() {
        s = "untitled".into();
    }
    const RESERVED: [&str; 22] = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1",
        "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if RESERVED.contains(&s.to_lowercase().as_str()) {
        s.push('_');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> NoteStore {
        let dir = std::env::temp_dir().join(format!("nn-test-{}-{}", std::process::id(), rand_suffix()));
        NoteStore::new(dir)
    }
    fn rand_suffix() -> u128 {
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn sanitizes() {
        assert_eq!(sanitize_name("a/b:c?"), "a-b-c-");
        assert_eq!(sanitize_name("  hello.  "), "hello");
        assert_eq!(sanitize_name("CON"), "CON_");
        assert_eq!(sanitize_name(""), "untitled");
    }

    #[test]
    fn create_search_rename_delete() {
        let mut s = temp_store();
        let p = s.create("Meeting notes").unwrap();
        s.save(&p, "- [ ] buy milk\nproject alpha").unwrap();
        s.create("Shopping").unwrap();
        assert_eq!(s.notes.len(), 2);

        let hits = s.search("meet");
        assert_eq!(s.notes[hits[0].index].name, "Meeting notes");
        let hits = s.search("mtg nts");
        assert_eq!(s.notes[hits[0].index].name, "Meeting notes");
        let hits = s.search("alpha");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet.as_deref(), Some("project alpha"));
        assert!(s.find_exact("shopping").is_some());

        let p2 = s.rename(&p, "Standup").unwrap();
        assert!(p2.ends_with("Standup.md"));
        assert!(s.find_exact("meeting notes").is_none());
        s.delete(&p2).unwrap();
        assert_eq!(s.notes.len(), 1);
        let _ = fs::remove_dir_all(&s.dir);
    }
}
