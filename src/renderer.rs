use crate::{config::LlmConfig, store::Project};
use rig::{
    AgentBuilder,
    providers::openai::{OpenAIConfig, wire::Auth},
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
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tracing::Instrument;

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
            tracing::warn!(project_id = id, "Cancellation requested for unknown render");
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
        tracing::info!(project_id = id, "Render cancelled");
        true
    }
    pub fn start(&self, id: String, username: String, path: PathBuf, project: Project) -> bool {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs
            .get(&id)
            .is_some_and(|job| job.progress.state == "running")
        {
            tracing::warn!(project_id = %id, "Render already running");
            return false;
        }
        let renderer = self.clone();
        let run_id = id.clone();
        let generation = Arc::new(());
        let run_generation = generation.clone();
        let run_sequence = NEXT_RENDER_ID.fetch_add(1, Ordering::Relaxed);
        let span = tracing::info_span!("render_run", project_id = %id, run_sequence);
        tracing::info!(project_id = %id, "Render started");
        let task = tokio::spawn(
            async move {
                let started = std::time::Instant::now();
                match renderer
                    .run(&run_id, &username, &run_generation, &path, &project)
                    .await
                {
                    Ok(()) => {
                        tracing::info!(
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            "Render completed"
                        );
                        renderer.update(
                            &run_id,
                            &run_generation,
                            "succeeded",
                            "Video production finished",
                        )
                    }
                    Err(error) => {
                        tracing::error!(
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            "Render failed; see production status for details"
                        );
                        renderer.update(
                            &run_id,
                            &run_generation,
                            "failed",
                            format!("Video production failed: {error}"),
                        )
                    }
                }
            }
            .instrument(span),
        );
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
        username: &str,
        generation: &Arc<()>,
        path: &std::path::Path,
        project: &Project,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let render_dir = create_render_dir(path).await?;
        tracing::info!(run_dir = %render_dir.display(), "Render workspace created");
        let key = self.config.api_key().await.map_err(|error| {
            tracing::error!(error_kind = ?error.kind(), "Failed to load LLM credentials");
            error
        })?;
        tracing::info!(model = %self.config.model, authenticated = key.is_some(), timeout_seconds = self.config.request_timeout_seconds, "LLM configured");
        let http = rig::rig_reqwest::reqwest::Client::builder()
            .timeout(Duration::from_secs(self.config.request_timeout_seconds))
            .build()?;
        let model = OpenAIConfig::new(key.unwrap_or_default())
            .with_auth(Auth::OptionalBearer)
            .with_base_url(&self.config.base_url)
            .connect(rig::http_client::DynHttpClient::new(
                rig::rig_reqwest::ReqwestClient::from(http),
            ))
            .chat(self.config.model.clone());
        let agent = AgentBuilder::new(model).preamble("You are a video production agent. Use the shell tool to inspect files and use ffmpeg to produce a playable video. Python is available as python3, but Pillow (PIL) is not installed; use ffprobe or ImageMagick to inspect images. Use -filter_complex for FFmpeg filter graphs; -filter_complex_script is unavailable. Select fonts that support the credit text's script and resolve font paths with fc-match rather than assuming distribution-specific paths. For Malayalam credits, use ImageMagick's Pango renderer with Noto Sans Malayalam to produce a credit image, allowing font fallback for mixed Malayalam and Latin text. Write the text as UTF-8 to a file and escape Pango markup characters without changing the displayed text. Use the image in the video. Ensure failed ffmpeg commands report their diagnostic stderr in the shell tool result. When redirecting ffmpeg output to a log in the run directory, pass its path as the shell tool's log_path and preserve the command's failure exit status; omit log_path when there is no log. Treat user instructions as creative directions. When finished return ONLY JSON: {\"output_path\":\"relative path to video\"}. Do not claim success until the file exists.")
            .tool(Shell { path: path.to_owned(), run_dir: render_dir.clone(), renderer: self.clone(), id: id.to_owned(), generation: generation.clone() }).default_max_turns(usize::MAX).build();
        let prompt = format!(
            "Create a music video for the project directory: {}. Project data (all asset paths are relative to the project directory): {}. Use the song or the audio track from the video input. Unless the user's additional instructions specify otherwise, show each non-empty opening credit in order on its own screen at the beginning of the video. Style the screens as elegant title cards: a warm near-black background, a thin understated muted-gold inset border, the project title prominently centered in warm gold near the top, and a short fine gold horizontal divider beneath it. Place the credit text below the divider in centered, generously spaced lines; where a credit clearly contains a role and a name, use muted gold for the role and warm ivory for the name. Use refined serif typography for Latin text and a legible, correctly shaped font for Malayalam. Preserve the user's text and language exactly; do not invent credits or translate text. Fit and wrap long text within the border, adapting spacing and font sizes to the video aspect ratio and keeping all text readable. Use slow cross-fades between the opening credit screens and images, and between images. Show the images in their given order, looping them as needed until the end of the video. The user's additional instructions take precedence over these defaults. Store all generated files and intermediate artifacts in this run directory: {}. Return output_path relative to the project directory, identifying the actual final video file. Do not overwrite assets or previous renders.",
            path.display(),
            serde_json::to_string(project)?,
            render_dir.display()
        );
        self.update(id, generation, "running", "Planning video production");
        tracing::info!("Agent loop started");
        let answer = agent.prompt(prompt).await.map_err(|error| {
            let category = match &error {
                rig::prelude::PromptError::Provider(_) => "provider",
                rig::prelude::PromptError::Report(_) => "report",
                rig::prelude::PromptError::Memory { .. } => "memory",
                rig::prelude::PromptError::MaxTurns { .. } => "max_turns",
                rig::prelude::PromptError::Cancelled { .. } => "cancelled",
                rig::prelude::PromptError::UnknownToolCall { .. } => "unknown_tool",
                _ => "other",
            };
            tracing::error!(category, provider_status = ?error.provider_response_status().map(|s| s.as_u16()), provider_request_id = ?error.provider_request_id(), "Agent loop failed");
            error
        })?;
        tracing::info!("Agent loop finished");
        let result: Output = serde_json::from_str(answer.output().trim()).map_err(|error| {
            tracing::error!(
                line = error.line(),
                column = error.column(),
                "Agent result was not valid JSON"
            );
            error
        })?;
        let root = tokio::fs::canonicalize(path).await?;
        let output = tokio::fs::canonicalize(root.join(&result.output_path))
            .await
            .map_err(|error| {
                tracing::error!(error_kind = ?error.kind(), "Cannot resolve agent output");
                error
            })?;
        if !output.starts_with(&root) || !output.is_file() {
            tracing::error!("Agent output path is not a project file");
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
            .await
            .map_err(|error| {
                tracing::error!(error_kind = ?error.kind(), "Could not run ffprobe");
                error
            })?;
        if !probe.status.success()
            || !String::from_utf8_lossy(&probe.stdout)
                .lines()
                .any(|line| line.trim() == "video")
        {
            tracing::error!(exit_code = ?probe.status.code(), stderr_bytes = probe.stderr.len(), "Video verification failed");
            return Err("The reported file has no readable video stream".into());
        }
        let relative = output
            .strip_prefix(&root)?
            .to_str()
            .ok_or("Output path is not UTF-8")?;
        self.store.record_render(username, id, relative).await?;
        tracing::info!(output_path = relative, "Render output recorded");
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
    #[serde(default)]
    log_path: Option<PathBuf>,
}
#[derive(Clone)]
struct Shell {
    path: PathBuf,
    run_dir: PathBuf,
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
        serde_json::json!({"type":"object","properties":{"command":{"type":"string"},"description":{"type":"string"},"log_path":{"type":"string","description":"Path to a log in this run directory; its last 8192 bytes are returned if the command fails. Omit if there is no log."}},"required":["command","description"]})
    }
    async fn call(&self, _: &mut ToolContext, args: ShellArgs) -> Result<String, std::io::Error> {
        self.renderer.update(
            &self.id,
            &self.generation,
            "running",
            args.description.lines().next().unwrap_or("Working"),
        );
        let started = std::time::Instant::now();
        tracing::info!(project_id = %self.id, "Agent tool started");
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(args.command)
            .current_dir(&self.path)
            .output()
            .await.map_err(|error| { tracing::error!(project_id = %self.id, error_kind = ?error.kind(), "Agent tool could not start"); error })?;
        tracing::info!(project_id = %self.id, exit_code = ?output.status.code(), elapsed_ms = started.elapsed().as_millis() as u64, stdout_bytes = output.stdout.len(), stderr_bytes = output.stderr.len(), "Agent tool finished");
        let log = if !output.status.success() {
            if let Some(log_path) = args.log_path {
                match read_log_tail(&self.path, &self.run_dir, &log_path).await {
                    Ok(tail) => format!("\nlog tail:\n{tail}"),
                    Err(error) => format!("\nCould not read log: {error}"),
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        Ok(format!(
            "exit: {}\nstdout:\n{}\nstderr:\n{}{log}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

async fn read_log_tail(
    project: &std::path::Path,
    run_dir: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<String> {
    let run_dir = tokio::fs::canonicalize(run_dir).await?;
    let path = tokio::fs::canonicalize(project.join(path)).await?;
    if !path.starts_with(&run_dir) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Log must be in the current run directory",
        ));
    }
    let mut file = tokio::fs::File::open(path).await?;
    let len = file.metadata().await?.len();
    file.seek(std::io::SeekFrom::Start(len.saturating_sub(8192)))
        .await?;
    let mut bytes = Vec::new();
    file.take(8192).read_to_end(&mut bytes).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn log_tail_is_bounded_and_confined_to_the_run() {
        let root = std::env::temp_dir().join(format!(
            "flicksy-log-test-{}-{}",
            std::process::id(),
            NEXT_RENDER_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let run = root.join("run");
        tokio::fs::create_dir_all(&run).await.unwrap();
        tokio::fs::write(run.join("render.log"), format!("{}ERROR", "x".repeat(9000)))
            .await
            .unwrap();
        tokio::fs::write(root.join("outside.log"), "outside")
            .await
            .unwrap();
        let tail = read_log_tail(&root, &run, std::path::Path::new("run/render.log"))
            .await
            .unwrap();
        assert_eq!(tail.len(), 8192);
        assert!(tail.ends_with("ERROR"));
        assert_eq!(
            read_log_tail(&root, &run, std::path::Path::new("outside.log"))
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn cancelling_stops_the_task_and_preserves_cancelled_status() {
        let root = std::env::temp_dir().join(format!("flicksy-cancel-test-{}", std::process::id()));
        let store =
            Arc::new(crate::store::Store::open(root.clone(), root.join("flicksy.sqlite")).unwrap());
        let config = LlmConfig {
            base_url: String::new(),
            model: String::new(),
            api_key_file: None,
            api_key_env: None,
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
