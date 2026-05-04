use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::{Path, PathBuf};

enum Line {
    KeyValue(String, String),
    Other(String),
}

pub struct EnvFile {
    path: PathBuf,
    lines: Vec<Line>,
}

impl EnvFile {
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
        let lines = content
            .lines()
            .map(|line| {
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    if !key.is_empty() && !key.starts_with('#') {
                        let value = value.trim();
                        let value = if (value.starts_with('"') && value.ends_with('"'))
                            || (value.starts_with('\'') && value.ends_with('\''))
                        {
                            &value[1..value.len() - 1]
                        } else {
                            value
                        };
                        return Line::KeyValue(key.to_string(), value.to_string());
                    }
                }
                Line::Other(line.to_string())
            })
            .collect();
        Ok(Self {
            path: path.to_path_buf(),
            lines,
        })
    }

    pub fn get(&self, key: &str) -> &str {
        for line in &self.lines {
            if let Line::KeyValue(k, v) = line {
                if k == key {
                    return v;
                }
            }
        }
        ""
    }

    pub fn set(&mut self, key: &str, value: &str) {
        for line in &mut self.lines {
            if let Line::KeyValue(k, v) = line {
                if k == key {
                    *v = value.to_string();
                    return;
                }
            }
        }
        self.lines
            .push(Line::KeyValue(key.to_string(), value.to_string()));
    }

    pub fn save(&self) -> Result<(), String> {
        let mut content = String::new();
        for line in &self.lines {
            match line {
                Line::KeyValue(k, v) => {
                    if v.contains(':') || v.contains(' ') || v.contains('"') || v.contains('\'') {
                        writeln!(content, "{}=\"{}\"", k, v.replace('"', "\\\""))
                    } else {
                        writeln!(content, "{}={}", k, v)
                    }
                }
                Line::Other(s) => writeln!(content, "{}", s),
            }
            .unwrap();
        }
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, &content).map_err(|e| format!("Cannot write {}: {}", tmp.display(), e))?;
        fs::rename(&tmp, &self.path).map_err(|e| {
            format!(
                "Cannot rename {} -> {}: {}",
                tmp.display(),
                self.path.display(),
                e
            )
        })?;
        Ok(())
    }

    pub fn reload(&mut self) -> Result<(), String> {
        let reloaded = Self::load(&self.path)?;
        self.lines = reloaded.lines;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn round_trip_preserves_comments() {
        let dir = std::env::temp_dir().join("nifty-test-roundtrip");
        let _ = fs::remove_file(&dir);
        let content = "# comment\nFOO=bar\n\nBAZ=qux\n";
        fs::write(&dir, content).unwrap();

        let env = EnvFile::load(&dir).unwrap();
        assert_eq!(env.get("FOO"), "bar");
        assert_eq!(env.get("BAZ"), "qux");
        assert_eq!(env.get("MISSING"), "");

        env.save().unwrap();
        let saved = fs::read_to_string(&dir).unwrap();
        assert_eq!(saved, content);
        fs::remove_file(&dir).unwrap();
    }

    #[test]
    fn set_updates_existing_key() {
        let dir = std::env::temp_dir().join("nifty-test-set");
        let _ = fs::remove_file(&dir);
        fs::write(&dir, "A=1\nB=2\n").unwrap();

        let mut env = EnvFile::load(&dir).unwrap();
        env.set("A", "99");
        env.save().unwrap();

        let saved = fs::read_to_string(&dir).unwrap();
        assert_eq!(saved, "A=99\nB=2\n");
        fs::remove_file(&dir).unwrap();
    }

    #[test]
    fn set_appends_new_key() {
        let dir = std::env::temp_dir().join("nifty-test-append");
        let _ = fs::remove_file(&dir);
        fs::write(&dir, "A=1\n").unwrap();

        let mut env = EnvFile::load(&dir).unwrap();
        env.set("B", "2");
        env.save().unwrap();

        let saved = fs::read_to_string(&dir).unwrap();
        assert_eq!(saved, "A=1\nB=2\n");
        fs::remove_file(&dir).unwrap();
    }
}
