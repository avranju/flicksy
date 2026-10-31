use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

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
    database: PathBuf,
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
    pub fn open(root: PathBuf, database: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&root)?;
        let root = std::fs::canonicalize(root)?;
        let store = Self {
            root,
            database,
            lock: Mutex::new(()),
        };
        store.initialize_schema()?;
        tracing::info!(root = %store.root.display(), "Project store ready");
        Ok(store)
    }
    fn connect(&self) -> io::Result<Connection> {
        let connection = Connection::open(&self.database).map_err(db_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(db_error)?;
        Ok(connection)
    }
    fn initialize_schema(&self) -> io::Result<()> {
        let connection = self.connect()?;
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='projects')",
                [],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if !exists && self.root.join("index.json").exists() {
            return Err(io::Error::other(
                "Legacy projects require the separate project migration before starting the server",
            ));
        }
        if exists {
            let mut statement = connection
                .prepare("PRAGMA table_info(projects)")
                .map_err(db_error)?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(db_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_error)?;
            if !columns.iter().any(|column| column == "username") {
                return Err(io::Error::other(
                    "Existing projects table has an unsupported schema",
                ));
            }
        }
        connection.execute_batch("CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY NOT NULL, username TEXT NOT NULL REFERENCES users(username) ON DELETE RESTRICT, title TEXT NOT NULL, song_json TEXT NOT NULL, images_json TEXT NOT NULL, image_number_high_water TEXT NOT NULL, credits_json TEXT NOT NULL, instructions TEXT NOT NULL, renders_json TEXT NOT NULL); CREATE INDEX IF NOT EXISTS projects_username ON projects(username);").map_err(db_error)?;
        Ok(())
    }
    pub fn project_path(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }
    pub async fn list_projects(&self, username: &str) -> io::Result<Vec<ProjectSummary>> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare("SELECT id, title FROM projects WHERE username = ?1 ORDER BY rowid")
            .map_err(db_error)?;
        let projects = statement
            .query_map([username], |row| {
                Ok(ProjectSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            })
            .map_err(db_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_error)?;
        Ok(projects)
    }
    #[tracing::instrument(level = "debug", skip_all, fields(project_id = id), err)]
    pub async fn load_project(&self, username: &str, id: &str) -> io::Result<Project> {
        if !safe_id(id) {
            return Err(not_found());
        }
        load_project(&self.connect()?, username, id)
    }
    #[tracing::instrument(skip_all, fields(project_id = id), err)]
    pub async fn record_render(&self, username: &str, id: &str, output: &str) -> io::Result<()> {
        let _guard = self.lock.lock().await;
        if !safe_id(id) {
            return Err(not_found());
        }
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let mut project = load_project(&transaction, username, id)?;
        let number = project
            .renders
            .iter()
            .map(|render| render.number)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| io::Error::other("Render number overflow"))?;
        project.renders.push(Render {
            number,
            path: output.to_owned(),
        });
        update_project_row(&transaction, username, id, &project)?;
        transaction.commit().map_err(db_error)?;
        tracing::info!(render_number = number, "Render metadata saved");
        Ok(())
    }
    #[tracing::instrument(skip_all, fields(project_id = id), err)]
    pub async fn update_project(
        &self,
        username: &str,
        id: &str,
        mut pending: PendingProject,
        project: &mut Project,
    ) -> io::Result<()> {
        let _guard = self.lock.lock().await;
        if !safe_id(id) {
            return Err(not_found());
        }
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        project.renders = load_project(&transaction, username, id)?.renders;
        let directory = self.root.join(id);
        let uploads = directory.join("uploads");
        std::fs::create_dir_all(&uploads)?;
        let destination = uploads.join(&pending.id);
        std::fs::rename(&pending.path, &destination)?;
        pending.path = destination;
        update_project_row(&transaction, username, id, project)?;
        transaction.commit().map_err(db_error)?;
        pending.committed = true;
        drop(pending);
        tracing::info!("Project updated");
        Ok(())
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
        username: &str,
        project: &Project,
    ) -> io::Result<String> {
        let _guard = self.lock.lock().await;
        let connection = self.connect()?;
        insert_project(&connection, &pending.id, username, project)?;
        pending.committed = true;
        tracing::info!("Project created");
        Ok(pending.id.clone())
    }
}
fn db_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
fn not_found() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "Project not found")
}
fn safe_id(id: &str) -> bool {
    let mut components = Path::new(id).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}
fn json(value: &impl Serialize) -> io::Result<String> {
    serde_json::to_string(value).map_err(io::Error::other)
}
fn insert_project(
    connection: &Connection,
    id: &str,
    username: &str,
    project: &Project,
) -> io::Result<()> {
    connection.execute("INSERT INTO projects (id,username,title,song_json,images_json,image_number_high_water,credits_json,instructions,renders_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![id, username, project.title, json(&project.song)?, json(&project.images)?, project.image_number_high_water.to_string(), json(&project.credits)?, project.instructions, json(&project.renders)?]).map_err(db_error)?;
    Ok(())
}
fn update_project_row(
    connection: &Connection,
    username: &str,
    id: &str,
    project: &Project,
) -> io::Result<()> {
    let changed = connection.execute("UPDATE projects SET title=?2,song_json=?3,images_json=?4,image_number_high_water=?5,credits_json=?6,instructions=?7,renders_json=?8 WHERE id=?1 AND username=?9", params![id, project.title, json(&project.song)?, json(&project.images)?, project.image_number_high_water.to_string(), json(&project.credits)?, project.instructions, json(&project.renders)?, username]).map_err(db_error)?;
    if changed == 0 {
        return Err(not_found());
    }
    Ok(())
}
fn load_project(connection: &Connection, username: &str, id: &str) -> io::Result<Project> {
    let data: Option<(String, String, String, String, String, String, String)> = connection.query_row("SELECT title,song_json,images_json,image_number_high_water,credits_json,instructions,renders_json FROM projects WHERE id=?1 AND username=?2", params![id, username], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?))).optional().map_err(db_error)?;
    let (title, song, images, high_water, credits, instructions, renders) =
        data.ok_or_else(not_found)?;
    Ok(Project {
        title,
        song: serde_json::from_str(&song).map_err(io::Error::other)?,
        images: serde_json::from_str(&images).map_err(io::Error::other)?,
        image_number_high_water: high_water.parse().map_err(io::Error::other)?,
        credits: serde_json::from_str(&credits).map_err(io::Error::other)?,
        instructions,
        renders: serde_json::from_str(&renders).map_err(io::Error::other)?,
    })
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

    fn test_store(root: &Path) -> Store {
        std::fs::create_dir_all(root).unwrap();
        let db = root.join("flicksy.sqlite");
        let connection = Connection::open(&db).unwrap();
        connection.execute_batch("CREATE TABLE users (username TEXT PRIMARY KEY NOT NULL, password_hash TEXT NOT NULL); INSERT INTO users VALUES ('alice','hash'), ('bob','hash');").unwrap();
        Store::open(root.to_owned(), db).unwrap()
    }

    #[tokio::test]
    async fn project_access_is_scoped_to_owner_and_owner_deletion_is_blocked() {
        let root = std::env::temp_dir().join(format!(
            "flicksy-owner-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let store = test_store(&root);
        let id = store
            .commit(
                store.begin().unwrap(),
                "alice",
                &Project {
                    title: "Private".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(store.list_projects("alice").await.unwrap().len(), 1);
        assert!(store.list_projects("bob").await.unwrap().is_empty());
        assert_eq!(
            store.load_project("bob", &id).await.unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(store.record_render("bob", &id, "file.mp4").await.is_err());
        let mut project = Project {
            title: "Tampered".into(),
            ..Default::default()
        };
        assert!(
            store
                .update_project("bob", &id, store.begin().unwrap(), &mut project)
                .await
                .is_err()
        );
        assert_eq!(
            store.load_project("alice", &id).await.unwrap().title,
            "Private"
        );
        assert!(
            store
                .connect()
                .unwrap()
                .execute("DELETE FROM users WHERE username = ?1", ["alice"])
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn failed_update_removes_new_uploads_and_preserves_project() {
        let root = std::env::temp_dir().join(format!(
            "flicksy-update-fail-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let store = test_store(&root);
        let id = store
            .commit(
                store.begin().unwrap(),
                "alice",
                &Project {
                    title: "Original".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        store.connect().unwrap().execute_batch("CREATE TRIGGER fail_update BEFORE UPDATE ON projects BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        let pending = store.begin().unwrap();
        let destination = root.join(&id).join("uploads").join(&pending.id);
        std::fs::write(pending.path.join("image.jpg"), b"image").unwrap();
        let mut edited = Project {
            title: "Changed".into(),
            ..Default::default()
        };
        assert!(
            store
                .update_project("alice", &id, pending, &mut edited)
                .await
                .is_err()
        );
        assert!(!destination.exists());
        assert_eq!(
            store.load_project("alice", &id).await.unwrap().title,
            "Original"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn editing_keeps_render_history_and_updates_index() {
        let root = std::env::temp_dir().join(format!(
            "flicksy-edit-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let store = test_store(&root);
        assert!(store.list_projects("alice").await.unwrap().is_empty());
        let initial = Project {
            title: "Old".into(),
            ..Default::default()
        };
        let id = store
            .commit(store.begin().unwrap(), "alice", &initial)
            .await
            .unwrap();
        store
            .record_render("alice", &id, "renders/first.mp4")
            .await
            .unwrap();
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
        store
            .update_project("alice", &id, pending, &mut edit)
            .await
            .unwrap();
        let projects = store.list_projects("alice").await.unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, id);
        assert_eq!(projects[0].title, "New");
        let loaded = store.load_project("alice", &id).await.unwrap();
        assert_eq!(loaded.title, "New");
        assert_eq!(loaded.renders[0].path, "renders/first.mp4");
        assert_eq!(loaded.images[0].number, 7);
        assert_eq!(
            std::fs::read(root.join(&id).join(uploaded)).unwrap(),
            b"new image"
        );
        assert_eq!(store.load_project("alice", &id).await.unwrap().title, "New");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn commits_concurrently_and_cleans_up_abandoned_projects() {
        let root = std::env::temp_dir().join(format!(
            "flicksy-store-test-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let store = test_store(&root);
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
            store.commit(first, "alice", &project),
            store.commit(second, "alice", &project)
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        let index = store.list_projects("alice").await.unwrap();
        assert_eq!(index.len(), 2);
        for id in [a, b] {
            assert!(index.iter().any(|entry| entry.id == id));
            let saved = store.load_project("alice", &id).await.unwrap();
            assert_eq!(saved.title, "Test");
            store
                .record_render("alice", &id, "output.mp4")
                .await
                .unwrap();
            let updated = store.load_project("alice", &id).await.unwrap();
            assert_eq!(updated.renders.len(), 1);
            assert_eq!(updated.renders[0].path, "output.mp4");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
