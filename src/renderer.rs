use crate::{config::LlmConfig, store::Project};
use rig::{
    AgentBuilder,
    providers::openai::OpenAIConfig,
    tool::{Tool, ToolContext},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static NEXT_RENDER_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize)]
pub struct Progress {
    pub state: String,
    pub message: String,
}
struct Job {
    progress: Progress,
    task: tokio::task::AbortHandle,
    generation: Arc<()>,
}
#[derive(Clone)]
pub struct Renderer {
    config: LlmConfig,
    store: Arc<crate::store::Store>,
    jobs: Arc<Mutex<HashMap<String, Job>>>,
}
impl Renderer {
    pub fn new(config: LlmConfig, store: Arc<crate::store::Store>) -> Self {
        Self {
            config,
            store,
            jobs: Arc::default(),
        }
    }
    fn update(&self, id: &str, generation: &Arc<()>, state: &str, message: impl Into<String>) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(id) {
            if job.progress.state == "running" && Arc::ptr_eq(&job.generation, generation) {
                job.progress = Progress {
                    state: state.into(),
                    message: message.into(),
                };
            }
        }
    }
    pub fn status(&self, id: &str) -> Option<Progress> {
        self.jobs
            .lock()
            .unwrap()
            .get(id)
            .map(|job| job.progress.clone())
    }
    pub fn cancel(&self, id: &str) -> bool {
        let mut jobs = self.jobs.lock().unwrap();
        let Some(job) = jobs.get_mut(id) else {
            return false;
        };
        if job.progress.state != "running" {
            return false;
        }
        job.progress = Progress {
            state: "cancelled".into(),
            message: "Video production cancelled.".into(),
        };
        job.task.abort();
        true
    }
    pub fn start(&self, id: String, path: PathBuf, project: Project) -> bool {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs
            .get(&id)
            .is_some_and(|job| job.progress.state == "running")
        {
            return false;
        }
        let renderer = self.clone();
        let run_id = id.clone();
        let generation = Arc::new(());
        let run_generation = generation.clone();
        let task = tokio::spawn(async move {
            match renderer
                .run(&run_id, &run_generation, &path, &project)
                .await
            {
                Ok(()) => renderer.update(
                    &run_id,
                    &run_generation,
                    "succeeded",
                    "Video production finished",
                ),
                Err(error) => renderer.update(
                    &run_id,
                    &run_generation,
                    "failed",
                    format!("Video production failed: {error}"),
                ),
            }
        });
        jobs.insert(
            id,
            Job {
                progress: Progress {
                    state: "running".into(),
                    message: "Preparing video production".into(),
                },
                task: task.abort_handle(),
                generation,
            },
        );
        true
    }
    async fn run(
        &self,
        id: &str,
        generation: &Arc<()>,
        path: &std::path::Path,
        project: &Project,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let render_dir = create_render_dir(path).await?;
        let key = tokio::fs::read_to_string(&self.config.api_key_file).await?;
        let http = rig::rig_reqwest::reqwest::Client::builder()
            .timeout(Duration::from_secs(self.config.request_timeout_seconds))
            .build()?;
        let model = OpenAIConfig::new(key.trim().to_owned())
            .with_base_url(&self.config.base_url)
            .connect(rig::http_client::DynHttpClient::new(
                rig::rig_reqwest::ReqwestClient::from(http),
            ))
            .chat(self.config.model.clone());
        let agent = AgentBuilder::new(model).preamble("You are a video production agent. Use the shell tool to inspect files and use ffmpeg to produce a playable video. Treat user instructions as creative directions. When finished return ONLY JSON: {\"output_path\":\"relative path to video\"}. Do not claim success until the file exists.")
            .tool(Shell { path: path.to_owned(), renderer: self.clone(), id: id.to_owned(), generation: generation.clone() }).default_max_turns(usize::MAX).build();
        let prompt = format!(
            "Create a music video for the project directory: {}. Project data (all asset paths are relative to the project directory): {}. Use the song or the audio track from the video input, images in their given order unless instructed otherwise, opening credits in order, and follow the additional instructions. Store all generated files and intermediate artifacts in this run directory: {}. Return output_path relative to the project directory, identifying the actual final video file. Do not overwrite assets or previous renders.",
            path.display(),
            serde_json::to_string(project)?,
            render_dir.display()
        );
        self.update(id, generation, "running", "Planning video production");
        let answer = agent.prompt(prompt).await?;
        let result: Output = serde_json::from_str(answer.output().trim())?;
        let root = tokio::fs::canonicalize(path).await?;
        let output = tokio::fs::canonicalize(root.join(&result.output_path)).await?;
        if !output.starts_with(&root) || !output.is_file() {
            return Err("The reported video must be a file in the project directory".into());
        }
        self.update(id, generation, "running", "Checking the finished video");
        let probe = tokio::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "csv=p=0",
            ])
            .arg(&output)
            .output()
            .await?;
        if !probe.status.success()
            || !String::from_utf8_lossy(&probe.stdout)
                .lines()
                .any(|line| line.trim() == "video")
        {
            return Err("The reported file has no readable video stream".into());
        }
        let relative = output
            .strip_prefix(&root)?
            .to_str()
            .ok_or("Output path is not UTF-8")?;
        self.store.record_render(id, relative).await?;
        Ok(())
    }
}
async fn create_render_dir(project: &std::path::Path) -> std::io::Result<PathBuf> {
    let renders = project.join("renders");
    tokio::fs::create_dir_all(&renders).await?;
    loop {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let id = NEXT_RENDER_ID.fetch_add(1, Ordering::Relaxed);
        let directory = renders.join(format!("run-{timestamp}-{}-{id}", std::process::id()));
        match tokio::fs::create_dir(&directory).await {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Output {
    output_path: String,
}
#[derive(Deserialize)]
struct ShellArgs {
    command: String,
    description: String,
}
#[derive(Clone)]
struct Shell {
    path: PathBuf,
    renderer: Renderer,
    id: String,
    generation: Arc<()>,
}
impl Tool for Shell {
    const NAME: &'static str = "shell";
    type Args = ShellArgs;
    type Output = String;
    type Error = std::io::Error;
    fn description(&self) -> String {
        "Run a shell command in the project directory. Include a short description of its purpose."
            .into()
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"command":{"type":"string"},"description":{"type":"string"}},"required":["command","description"]})
    }
    async fn call(&self, _: &mut ToolContext, args: ShellArgs) -> Result<String, std::io::Error> {
        self.renderer.update(
            &self.id,
            &self.generation,
            "running",
            args.description.lines().next().unwrap_or("Working"),
        );
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(args.command)
            .current_dir(&self.path)
            .output()
            .await?;
        Ok(format!(
            "exit: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_stops_the_task_and_preserves_cancelled_status() {
        let root = std::env::temp_dir().join(format!("flicksy-cancel-test-{}", std::process::id()));
        let store = Arc::new(crate::store::Store::open(root.clone()).unwrap());
        let config = LlmConfig {
            base_url: String::new(),
            model: String::new(),
            api_key_file: PathBuf::new(),
            request_timeout_seconds: 1,
        };
        let renderer = Renderer::new(config, store);
        let generation = Arc::new(());
        let task = tokio::spawn(std::future::pending::<()>());
        renderer.jobs.lock().unwrap().insert(
            "test".into(),
            Job {
                progress: Progress {
                    state: "running".into(),
                    message: "Working".into(),
                },
                task: task.abort_handle(),
                generation: generation.clone(),
            },
        );
        assert!(renderer.cancel("test"));
        assert!(task.await.unwrap_err().is_cancelled());
        renderer.update("test", &generation, "succeeded", "Late update");
        assert_eq!(renderer.status("test").unwrap().state, "cancelled");
        assert!(!renderer.cancel("test"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
