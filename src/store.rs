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
#[derive(Debug, Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub title: String,
}
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Project {
    pub title: String,
    pub song: Option<Asset>,
    pub images: Vec<Image>,
    #[serde(default)]
    pub image_number_high_water: u64,
    pub credits: Vec<String>,
    pub instructions: String,
    #[serde(default)]
    pub renders: Vec<Render>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
            if let Err(error) = std::fs::remove_dir_all(&self.path) {
                tracing::warn!(project_id = %self.id, error_kind = ?error.kind(), "Failed to clean up pending project");
            } else {
                tracing::debug!(project_id = %self.id, "Pending project removed");
            }
        }
    }
}

impl Store {
    #[tracing::instrument(skip_all, fields(root = %root.display()), err)]
    pub fn open(root: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&root)?;
        let root = std::fs::canonicalize(root)?;
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
        tracing::info!(root = %root.display(), "Project store ready");
        Ok(Self {
            root,
            lock: Mutex::new(()),
        })
    }
    pub fn project_path(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }
    pub async fn list_projects(&self) -> io::Result<Vec<ProjectSummary>> {
        let _guard = self.lock.lock().await;
        Ok(read_index(&self.root.join("index.json"))?
            .projects
            .into_iter()
            .filter(|entry| entry.id == entry.path)
            .map(|entry| ProjectSummary {
                id: entry.id,
                title: entry.title,
            })
            .collect())
    }
    #[tracing::instrument(level = "debug", skip_all, fields(project_id = id), err)]
    pub async fn load_project(&self, id: &str) -> io::Result<Project> {
        let _guard = self.lock.lock().await;
        let index = read_index(&self.root.join("index.json"))?;
        let entry = index
            .projects
            .iter()
            .find(|entry| entry.id == id && entry.path == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Project not found"))?;
        serde_json::from_reader(std::fs::File::open(
            self.root.join(&entry.path).join("project.json"),
        )?)
        .map_err(io::Error::other)
    }
    #[tracing::instrument(skip_all, fields(project_id = id), err)]
    pub async fn record_render(&self, id: &str, output: &str) -> io::Result<()> {
        let _guard = self.lock.lock().await;
        let directory = self.root.join(id);
        let metadata = directory.join("project.json");
        let mut project: Project =
            serde_json::from_reader(std::fs::File::open(&metadata)?).map_err(io::Error::other)?;
        let number = project
            .renders
            .iter()
            .map(|render| render.number)
            .max()
            .unwrap_or(0)
            + 1;
        project.renders.push(Render {
            number,
            path: output.to_owned(),
        });
        let temp = directory.join(format!(
            ".project-{}-{}.tmp",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        if let Err(error) =
            write_json(&temp, &project).and_then(|()| std::fs::rename(&temp, &metadata))
        {
            let _ = std::fs::remove_file(temp);
            return Err(error);
        }
        tracing::info!(render_number = number, "Render metadata saved");
        Ok(())
    }
    #[tracing::instrument(skip_all, fields(project_id = id), err)]
    pub async fn update_project(
        &self,
        id: &str,
        mut pending: PendingProject,
        project: &mut Project,
    ) -> io::Result<()> {
        let _guard = self.lock.lock().await;
        let index_path = self.root.join("index.json");
        let mut index = read_index(&index_path)?;
        let entry = index
            .projects
            .iter_mut()
            .find(|e| e.id == id && e.path == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Project not found"))?;
        let directory = self.root.join(id);
        let current: Project =
            serde_json::from_reader(std::fs::File::open(directory.join("project.json"))?)
                .map_err(io::Error::other)?;
        project.renders = current.renders;
        let uploads = directory.join("uploads");
        std::fs::create_dir_all(&uploads)?;
        let destination = uploads.join(&pending.id);
        std::fs::rename(&pending.path, &destination)?;
        pending.path = destination.clone();
        let project_temp = directory.join(format!(".project-{}.tmp", pending.id));
        let index_temp = self.root.join(format!(".index-{}.tmp", pending.id));
        entry.title = project.title.clone();
        let result = (|| {
            write_json(&project_temp, project)?;
            write_json(&index_temp, &index)?;
            std::fs::rename(&project_temp, directory.join("project.json"))?;
            // Preserve uploaded files once project metadata references them.
            pending.committed = true;
            std::fs::rename(&index_temp, &index_path)?;
            Ok(())
        })();
        let _ = std::fs::remove_file(project_temp);
        let _ = std::fs::remove_file(index_temp);
        if result.is_ok() {
            tracing::info!("Project updated");
        }
        result
    }
    #[tracing::instrument(level = "debug", skip_all, err)]
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
    #[tracing::instrument(skip_all, fields(project_id = %pending.id), err)]
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
        tracing::info!("Project created");
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
    async fn editing_keeps_render_history_and_updates_index() {
        let root = std::env::temp_dir().join(format!(
            "flicksy-edit-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let store = Store::open(root.clone()).unwrap();
        assert!(store.list_projects().await.unwrap().is_empty());
        let initial = Project {
            title: "Old".into(),
            ..Default::default()
        };
        let id = store
            .commit(store.begin().unwrap(), &initial)
            .await
            .unwrap();
        store.record_render(&id, "renders/first.mp4").await.unwrap();
        let pending = store.begin().unwrap();
        std::fs::write(pending.path.join("image.jpg"), b"new image").unwrap();
        let uploaded = format!("uploads/{}/image.jpg", pending.id);
        let mut edit = Project {
            title: "New".into(),
            images: vec![Image {
                number: 7,
                asset: Asset {
                    original_name: "photo.jpg".into(),
                    path: uploaded.clone(),
                },
            }],
            ..Default::default()
        };
        store.update_project(&id, pending, &mut edit).await.unwrap();
        let projects = store.list_projects().await.unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, id);
        assert_eq!(projects[0].title, "New");
        let loaded = store.load_project(&id).await.unwrap();
        assert_eq!(loaded.title, "New");
        assert_eq!(loaded.renders[0].path, "renders/first.mp4");
        assert_eq!(loaded.images[0].number, 7);
        assert_eq!(
            std::fs::read(root.join(&id).join(uploaded)).unwrap(),
            b"new image"
        );
        assert_eq!(
            read_index(&root.join("index.json")).unwrap().projects[0].title,
            "New"
        );
        std::fs::remove_dir_all(root).unwrap();
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
                std::fs::File::open(root.join(&id).join("project.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(saved.title, "Test");
            store.record_render(&id, "output.mp4").await.unwrap();
            let updated = store.load_project(&id).await.unwrap();
            assert_eq!(updated.renders.len(), 1);
            assert_eq!(updated.renders[0].path, "output.mp4");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
