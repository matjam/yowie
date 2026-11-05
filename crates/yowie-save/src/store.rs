use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use crossbeam_channel::Sender;
use directories::ProjectDirs;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::migrate::{open_envelope, upgrade};
use crate::worker::{Job, run, write_atomic};
use crate::{Migration, SaveError, SaveHandle};

#[derive(Serialize)]
struct Envelope<'a, T> {
    version: u32,
    data: &'a T,
}

/// A directory of named JSON save slots. See the crate documentation.
pub struct Store {
    dir: PathBuf,
    version: u32,
    migration: Option<Migration>,
    jobs: Option<Sender<Job>>,
    worker: Option<JoinHandle<()>>,
}

fn check_name(name: &str) -> Result<(), SaveError> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.starts_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c));
    if bad {
        return Err(SaveError::InvalidName(name.to_string()));
    }
    Ok(())
}

impl Store {
    /// Opens the data directory for an application, creating it if needed.
    /// The three names follow `directories::ProjectDirs`: a reverse-domain
    /// qualifier (`"com"`), the organisation (`"Example"`) and the
    /// application (`"My Game"`).
    pub fn open(qualifier: &str, organisation: &str, application: &str) -> Result<Self, SaveError> {
        let dirs =
            ProjectDirs::from(qualifier, organisation, application).ok_or(SaveError::NoDataDir)?;
        Self::at(dirs.data_dir())
    }

    /// Opens a store in an explicit directory, creating it if needed.
    pub fn at(dir: impl AsRef<Path>) -> Result<Self, SaveError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir).map_err(|source| SaveError::Io {
            name: dir.display().to_string(),
            source,
        })?;
        let (tx, rx) = crossbeam_channel::unbounded();
        let worker_dir = dir.clone();
        let worker = std::thread::Builder::new()
            .name("yowie-save".into())
            .spawn(move || run(worker_dir, rx))
            .map_err(|source| SaveError::Io {
                name: dir.display().to_string(),
                source,
            })?;
        Ok(Self {
            dir,
            version: 1,
            migration: None,
            jobs: Some(tx),
            worker: Some(worker),
        })
    }

    /// Sets the version recorded in saves written from now on and expected
    /// when reading. The default is 1.
    pub fn with_version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// Sets the hook that upgrades older saves. See [`Migration`].
    pub fn migrate_with(
        mut self,
        hook: impl Fn(u32, Value) -> Result<Value, String> + Send + Sync + 'static,
    ) -> Self {
        self.migration = Some(Box::new(hook));
        self
    }

    /// The directory the saves live in.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    fn encode<T: Serialize>(&self, name: &str, value: &T) -> Result<Vec<u8>, SaveError> {
        check_name(name)?;
        serde_json::to_vec_pretty(&Envelope {
            version: self.version,
            data: value,
        })
        .map_err(|source| SaveError::Json {
            name: name.to_string(),
            source,
        })
    }

    /// Writes a slot and returns when it is on disk. Waits for earlier
    /// background saves first so slots never go backwards.
    pub fn write<T: Serialize>(&self, name: &str, value: &T) -> Result<(), SaveError> {
        let bytes = self.encode(name, value)?;
        self.flush();
        write_atomic(&self.dir, name, &bytes)
    }

    /// Encodes a slot now and writes it on the worker thread. Poll the
    /// returned handle for the outcome. Saves run in the order requested.
    pub fn write_async<T: Serialize>(&self, name: &str, value: &T) -> SaveHandle {
        let bytes = match self.encode(name, value) {
            Ok(b) => b,
            Err(e) => return SaveHandle::finished(Err(e)),
        };
        let (reply, rx) = crossbeam_channel::bounded(1);
        let job = Job::Write {
            name: name.to_string(),
            bytes,
            reply,
        };
        match &self.jobs {
            Some(tx) if tx.send(job).is_ok() => SaveHandle::new(rx),
            _ => SaveHandle::finished(Err(SaveError::WorkerGone)),
        }
    }

    /// Blocks until every queued background save has finished.
    pub fn flush(&self) {
        let (reply, rx) = crossbeam_channel::bounded(1);
        if let Some(tx) = &self.jobs
            && tx.send(Job::Barrier(reply)).is_ok()
        {
            let _ = rx.recv();
        }
    }

    /// Reads a slot, upgrading it with the migration hook when it was
    /// written at an older version. Waits for queued saves first.
    pub fn read<T: DeserializeOwned>(&self, name: &str) -> Result<T, SaveError> {
        check_name(name)?;
        self.flush();
        let json_err = |source| SaveError::Json {
            name: name.to_string(),
            source,
        };
        let bytes = fs::read(self.file(name)).map_err(|e| match e.kind() {
            ErrorKind::NotFound => SaveError::NotFound(name.to_string()),
            _ => SaveError::Io {
                name: name.to_string(),
                source: e,
            },
        })?;
        let value: Value = serde_json::from_slice(&bytes).map_err(json_err)?;
        let (version, data) = open_envelope(value);
        let data = upgrade(name, version, data, self.version, self.migration.as_ref())?;
        serde_json::from_value(data).map_err(json_err)
    }

    /// Reads a slot, or returns `default` when it does not exist. Other
    /// errors, such as a corrupt file, are still reported. Use it for
    /// settings.
    pub fn load_or<T: DeserializeOwned>(&self, name: &str, default: T) -> Result<T, SaveError> {
        match self.read(name) {
            Err(SaveError::NotFound(_)) => Ok(default),
            other => other,
        }
    }

    /// Returns true when the slot exists.
    pub fn exists(&self, name: &str) -> bool {
        check_name(name).is_ok() && {
            self.flush();
            self.file(name).is_file()
        }
    }

    /// Deletes a slot. Deleting a slot that does not exist succeeds.
    pub fn delete(&self, name: &str) -> Result<(), SaveError> {
        check_name(name)?;
        self.flush();
        match fs::remove_file(self.file(name)) {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(SaveError::Io {
                name: name.to_string(),
                source: e,
            }),
            _ => Ok(()),
        }
    }

    /// Lists the slot names, sorted.
    pub fn list(&self) -> Result<Vec<String>, SaveError> {
        self.flush();
        let io = |source| SaveError::Io {
            name: self.dir.display().to_string(),
            source,
        };
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.dir).map_err(io)? {
            let entry = entry.map_err(io)?;
            let file = entry.file_name();
            let file = file.to_string_lossy();
            if let Some(stem) = file.strip_suffix(".json")
                && !stem.starts_with('.')
                && entry.path().is_file()
            {
                names.push(stem.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.json"))
    }
}

impl Drop for Store {
    /// Waits for queued background saves, then stops the worker.
    fn drop(&mut self) {
        self.jobs = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;
    use std::time::{Duration, Instant};

    #[derive(Serialize, Deserialize, Debug, PartialEq, Default, Clone)]
    struct Game {
        level: u32,
        name: String,
    }

    fn game() -> Game {
        Game {
            level: 7,
            name: "Ada".into(),
        }
    }

    #[test]
    fn write_read_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path()).unwrap();
        s.write("slot1", &game()).unwrap();
        assert_eq!(s.read::<Game>("slot1").unwrap(), game());
        assert!(s.exists("slot1") && !s.exists("slot2"));
        let text = fs::read_to_string(dir.path().join("slot1.json")).unwrap();
        assert!(text.contains("\"version\": 1"));
        // No temporary files are left behind.
        let names: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn missing_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path()).unwrap();
        assert!(matches!(s.read::<Game>("x"), Err(SaveError::NotFound(_))));
        assert_eq!(s.load_or("x", game()).unwrap(), game());
        fs::write(dir.path().join("bad.json"), "{nope").unwrap();
        assert!(matches!(
            s.load_or("bad", game()),
            Err(SaveError::Json { .. })
        ));
    }

    #[test]
    fn rejects_bad_names() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path()).unwrap();
        for name in ["", ".", "..", "a/b", "a\\b", ".hidden", "a:b", "x\0y"] {
            assert!(
                matches!(s.write(name, &1), Err(SaveError::InvalidName(_))),
                "{name:?}"
            );
            assert!(!s.exists(name));
        }
        assert!(matches!(
            s.write_async("a/b", &1).wait(),
            Err(SaveError::InvalidName(_))
        ));
    }

    #[test]
    fn list_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path()).unwrap();
        for n in ["b", "a", "c"] {
            s.write(n, &1).unwrap();
        }
        fs::write(dir.path().join("notes.txt"), "x").unwrap();
        assert_eq!(s.list().unwrap(), ["a", "b", "c"]);
        s.delete("b").unwrap();
        s.delete("b").unwrap();
        assert_eq!(s.list().unwrap(), ["a", "c"]);
    }

    #[test]
    fn async_save_completes_and_reads_wait_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::at(dir.path()).unwrap();
        let mut handle = s.write_async("slot", &game());
        let start = Instant::now();
        while !handle.is_done() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "save never finished"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(matches!(handle.poll(), Some(Ok(()))));
        assert!(handle.wait().is_ok());
        // Queued saves land in order, and a read sees the last one.
        for level in 0..20 {
            let _ = s.write_async(
                "slot",
                &Game {
                    level,
                    name: "x".into(),
                },
            );
        }
        assert_eq!(s.read::<Game>("slot").unwrap().level, 19);
    }

    #[test]
    fn drop_finishes_queued_saves() {
        let dir = tempfile::tempdir().unwrap();
        {
            let s = Store::at(dir.path()).unwrap();
            let _ = s.write_async("slot", &game());
        }
        let s = Store::at(dir.path()).unwrap();
        assert_eq!(s.read::<Game>("slot").unwrap(), game());
    }

    #[test]
    fn migrates_old_versions() {
        let dir = tempfile::tempdir().unwrap();
        // Version 0 was a bare object with "lvl"; version 1 renamed it.
        fs::write(dir.path().join("old.json"), r#"{"lvl": 3, "name": "Bo"}"#).unwrap();
        fs::write(
            dir.path().join("v1.json"),
            r#"{"version": 1, "data": {"level": 4, "name": "Cy"}}"#,
        )
        .unwrap();
        let s =
            Store::at(dir.path())
                .unwrap()
                .with_version(2)
                .migrate_with(|from, mut v| match from {
                    0 => {
                        if let Some(lvl) = v.get("lvl").cloned() {
                            v["level"] = lvl;
                        }
                        Ok(v)
                    }
                    1 => Ok(v),
                    _ => Err("unknown version".into()),
                });
        assert_eq!(
            s.read::<Game>("old").unwrap(),
            Game {
                level: 3,
                name: "Bo".into()
            }
        );
        assert_eq!(s.read::<Game>("v1").unwrap().name, "Cy");
        s.write("new", &game()).unwrap();
        let text = fs::read_to_string(dir.path().join("new.json")).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap()["version"],
            json!(2)
        );
    }

    #[test]
    fn newer_saves_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("f.json"), r#"{"version": 9, "data": {}}"#).unwrap();
        let s = Store::at(dir.path()).unwrap();
        assert!(matches!(
            s.read::<Game>("f"),
            Err(SaveError::TooNew {
                found: 9,
                supported: 1,
                ..
            })
        ));
    }
}
