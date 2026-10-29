use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Index {
    pub projects: Vec<IndexEntry>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub title: String,
    pub path: String,
}
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Project {
    pub title: String,
    pub song: Option<Asset>,
    pub images: Vec<Image>,
    pub credits: Vec<String>,
    pub instructions: String,
    #[serde(default)]
    pub renders: Vec<Render>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Asset {
    pub original_name: String,
    pub path: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Render {
    pub number: u64,
    // Relative to the project directory.
    pub path: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Image {
    pub number: u64,
    pub asset: Asset,
}

pub struct Store {
    root: PathBuf,
    lock: Mutex<()>,
}

// Removing an uncommitted directory also handles malformed or interrupted uploads.
pub struct PendingProject {
    pub path: PathBuf,
    pub id: String,
    committed: bool,
}
impl Drop for PendingProject {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

impl Store {
    pub fn open(root: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&root)?;
        let index = root.join("index.json");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&index)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(b"{\"projects\": []}\n")?;
                file.sync_all()?;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                read_index(&index)?;
            }
            Err(error) => return Err(error),
        }
        Ok(Self {
            root,
            lock: Mutex::new(()),
        })
    }
    pub fn begin(&self) -> io::Result<PendingProject> {
        loop {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos();
            let id = format!(
                "project-{now}-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            );
            let path = self.root.join(&id);
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(PendingProject {
                        path,
                        id,
                        committed: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }
    pub async fn commit(
        &self,
        mut pending: PendingProject,
        project: &Project,
    ) -> io::Result<String> {
        let _guard = self.lock.lock().await;
        write_json(&pending.path.join("project.json"), project)?;
        let index_path = self.root.join("index.json");
        let mut index = read_index(&index_path)?;
        index.projects.push(IndexEntry {
            id: pending.id.clone(),
            title: project.title.clone(),
            path: pending.id.clone(),
        });
        let temp = self.root.join(format!(".index-{}.tmp", pending.id));
        if let Err(error) =
            write_json(&temp, &index).and_then(|()| std::fs::rename(&temp, &index_path))
        {
            let _ = std::fs::remove_file(temp);
            return Err(error);
        }
        pending.committed = true;
        Ok(pending.id.clone())
    }
}
fn read_index(path: &Path) -> io::Result<Index> {
    serde_json::from_reader(std::fs::File::open(path)?).map_err(io::Error::other)
}
fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    serde_json::to_writer_pretty(&mut file, value).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_projects_default_to_no_renders() {
        let project: Project = serde_json::from_str(
            r#"{"title":"Test","song":null,"images":[],"credits":[],"instructions":""}"#,
        )
        .unwrap();
        assert!(project.renders.is_empty());
    }

    #[tokio::test]
    async fn commits_concurrently_and_cleans_up_abandoned_projects() {
        let root = std::env::temp_dir().join(format!(
            "flicksy-store-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let store = Store::open(root.clone()).unwrap();
        let abandoned = store.begin().unwrap();
        let abandoned_path = abandoned.path.clone();
        drop(abandoned);
        assert!(!abandoned_path.exists());
        let first = store.begin().unwrap();
        let second = store.begin().unwrap();
        let project = Project {
            title: "Test".into(),
            ..Default::default()
        };
        let (a, b) = tokio::join!(
            store.commit(first, &project),
            store.commit(second, &project)
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        let index = read_index(&root.join("index.json")).unwrap();
        assert_eq!(index.projects.len(), 2);
        for id in [a, b] {
            assert!(index.projects.iter().any(|entry| entry.path == id));
            let saved: Project = serde_json::from_reader(
                std::fs::File::open(root.join(id).join("project.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(saved.title, "Test");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
