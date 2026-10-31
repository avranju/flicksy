use clap::{Parser, Subcommand};
use std::{path::PathBuf, sync::Arc};
use tokio::io::AsyncWriteExt;
use topcoat::{
    Result,
    asset::{AssetBundle, RouterBuilderAssetExt},
    context::{Cx, app_context},
    font::fontsource::fontsource_font,
    router::{
        BodyLimit, RouterBuilderDiscoverExt, Slot,
        content::{Json, multipart::Multipart},
        error::bad_request,
        layout, module_router, page, query_params, route,
        tower::TowerLayer,
    },
    tailwind,
    view::{View, component, view},
};

mod accounts;
mod auth;
mod config;
mod preview;
mod production;
mod renderer;
mod store;
pub mod types;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Manage user accounts.
    Users {
        #[command(subcommand)]
        command: UsersCommand,
    },
}
#[derive(Subcommand)]
enum UsersCommand {
    /// Add a user (password is prompted without echo).
    Add { username: String },
    /// List users.
    List,
    /// Remove a user and revoke their sessions.
    Remove { username: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = config::Config::load(&args.config)?;
    let db_path = config
        .auth
        .database
        .clone()
        .unwrap_or_else(|| config.projects_dir.join("flicksy.sqlite"));
    let accounts = Arc::new(accounts::Accounts::open(
        &db_path,
        config.auth.secure_cookie,
    )?);
    if let Some(Command::Users { command }) = args.command {
        match command {
            UsersCommand::Add { username } => {
                let password = read_password()?;
                accounts.add(&username, &password)?;
                println!("Added user: {username}");
            }
            UsersCommand::List => {
                for username in accounts.list()? {
                    println!("{username}");
                }
            }
            UsersCommand::Remove { username } => {
                accounts.remove(&username)?;
                println!("Removed user: {username}");
            }
        }
        return Ok(());
    }
    let auth = auth::AuthLayer::new(accounts);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| config.tracing.level.filter().into()),
        )
        .with_target(true)
        .init();
    tracing::info!(config_path = %args.config.display(), "Starting Flicksy");
    let store = Arc::new(store::Store::open(config.projects_dir, db_path)?);
    let listener =
        tokio::net::TcpListener::bind((config.server.host.as_str(), config.server.port)).await?;
    tracing::info!(address = %listener.local_addr()?, "Server listening");
    let renderer = renderer::Renderer::new(config.llm, store.clone());
    Ok(topcoat::serve(
        listener,
        module_router!()
            .discover()
            .app_context(auth.clone())
            .app_context(store)
            .app_context(renderer)
            .layer(TowerLayer::new(tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(|request: &http::Request<topcoat::router::Body>| {
                    if request.uri().path() == "/render-status" {
                        tracing::debug_span!("http_request", method = %request.method(), path = request.uri().path())
                    } else {
                        tracing::info_span!("http_request", method = %request.method(), path = request.uri().path())
                    }
                })
                .on_response(|response: &http::Response<_>, latency: std::time::Duration, span: &tracing::Span| {
                    if span.is_disabled() && !response.status().is_server_error() { return; }
                    if response.status().is_server_error() {
                        tracing::error!(status = response.status().as_u16(), elapsed_ms = latency.as_millis() as u64, "Request failed");
                    } else {
                        tracing::info!(status = response.status().as_u16(), elapsed_ms = latency.as_millis() as u64, "Request completed");
                    }
                })))
            .layer(auth)
            .layer(BodyLimit::max(8192).at("/login"))
            .layer(BodyLimit::max(1024 * 1024 * 1024).at("/create-video"))
            .assets(AssetBundle::load()?)
            .build(),
    )
    .await?)
}

#[layout]
async fn layout(cx: &Cx, slot: Slot<'_>) -> Result<impl View> {
    let is_login = topcoat::router::request::uri(cx).path() == "/login";
    let username =
        topcoat::context::try_request_context::<auth::SignedInUser>(cx).map(|user| user.0.clone());
    Ok(view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <script src=(topcoat::asset::asset!("js/theme.js"))></script>
                topcoat::font::link(font: fontsource_font!(GEIST))
                <link rel="stylesheet" href=(tailwind::stylesheet!())>
                <title>"Flicksy"</title>
                <script src=(topcoat::asset::asset!("js/song-file.js")) defer="defer"></script>
                <script src=(topcoat::asset::asset!("js/images.js")) defer="defer"></script>
                <script src=(topcoat::asset::asset!("js/credits.js")) defer="defer"></script>
                <script src=(topcoat::asset::asset!("js/projects.js")) defer="defer"></script>
                <script src=(topcoat::asset::asset!("js/account-menu.js")) defer="defer"></script>
                <script
                    src=(topcoat::asset::asset!("js/form-validation.js"))
                    defer="defer"
                ></script>
                <script
                    src=(topcoat::asset::asset!("js/render-progress.js"))
                    defer="defer"
                ></script>
                topcoat::dev::script()
            </head>
            <body class="m-0 bg-background text-foreground">
                <header class="bg-card px-6 py-[26px]">
                    <div
                        class="mx-auto flex w-full max-w-[1192px] flex-wrap items-center gap-3"
                    >
                        <a href="/" aria-label="Flicksy - new project" class="flex shrink-0 items-center gap-3 no-underline text-inherit">
                            <div
                                class="flex size-9 items-center justify-center rounded-[11px] bg-[#6554e8] text-[23px] font-bold text-white"
                            >
                                "✦"
                            </div>
                            <span class="text-[26px] leading-[1.2] font-bold">
                                "flicksy"
                            </span>
                        </a>
                        <p class="text-[13px] text-muted-foreground">
                            "Turn songs and photos into videos."
                        </p>
                        <label class="ml-auto flex items-center gap-2 text-sm text-muted-foreground">
                            <span>"Theme"</span>
                            <select id="theme-select" class="cursor-pointer rounded-lg border border-border bg-card px-2 py-2 text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8]" aria-label="Color theme">
                                <option value="system">"System"</option>
                                <option value="light">"Light"</option>
                                <option value="dark">"Dark"</option>
                            </select>
                        </label>
                        if !is_login {
                        <button
                            id="projects-button"
                            type="button"
                            aria-haspopup="dialog"
                            class="rounded-xl bg-[#6554e8] px-4 py-2 text-sm font-bold text-white cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#5544d4] active:bg-[#4939bd]"
                        >
                            "Projects"
                        </button>
                        if let Some(username) = username {
                            <details id="account-menu" class="relative">
                                <summary class="flex w-[100px] cursor-pointer list-none items-center gap-1 rounded-xl border border-border px-4 py-2 text-sm font-bold hover:bg-surface focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] [&::-webkit-details-marker]:hidden" title=(username.clone())>
                                    <span class="min-w-0 flex-1 truncate">(username)</span>
                                    <span aria-hidden="true">"▾"</span>
                                </summary>
                                <form method="post" action="/logout" class="absolute top-full right-0 z-10 mt-2 min-w-[140px] rounded-xl border border-border bg-card p-1 shadow-lg">
                                    <button type="submit" class="w-full cursor-pointer rounded-lg px-3 py-2 text-left text-sm font-semibold hover:bg-surface focus-visible:outline-2 focus-visible:outline-[#6554e8]">"Log out"</button>
                                </form>
                            </details>
                        }
                        }
                    </div>
                </header>
                <dialog id="projects-dialog" aria-labelledby="projects-title" class="m-auto w-[calc(100%-3rem)] max-w-md overflow-hidden rounded-[20px] border-0 bg-card p-0 text-foreground shadow-xl backdrop:bg-black/50">
                    <div class="flex items-center justify-between gap-4 bg-background px-6 py-4">
                        <h2 id="projects-title" class="text-xl font-bold">"Projects"</h2>
                        <button id="projects-close" type="button" aria-label="Close projects" class="cursor-pointer rounded-lg px-2 py-1 text-xl text-muted-foreground hover:bg-background focus-visible:outline-2 focus-visible:outline-[#6554e8]">"×"</button>
                    </div>
                    <div id="projects-list" class="max-h-[60vh] overflow-y-auto bg-card px-6 py-2" aria-live="polite"></div>
                </dialog>
                (slot)
            </body>
        </html>
    })
}

#[route(GET "./projects")]
async fn projects(cx: &Cx) -> Result<Json<Vec<store::ProjectSummary>>> {
    Ok(Json(
        app_context::<Arc<store::Store>>(cx)
            .list_projects(auth::username(cx))
            .await?,
    ))
}

#[query_params(error = bad_request)]
struct HomeQuery {
    id: Option<String>,
}

#[page]
async fn home(cx: &Cx) -> Result<impl View> {
    let query = query_params::<HomeQuery>(cx)?;
    let project = if let Some(id) = &query.id {
        Some(
            app_context::<Arc<store::Store>>(cx)
                .load_project(auth::username(cx), id)
                .await
                .map_err(|_| bad_request("Project not found"))?,
        )
    } else {
        None
    };
    let title = project
        .as_ref()
        .map(|p| p.title.clone())
        .unwrap_or_default();
    let instructions = project
        .as_ref()
        .map(|p| p.instructions.clone())
        .unwrap_or_default();
    let credits = project
        .as_ref()
        .map(|p| p.credits.clone())
        .unwrap_or_default();
    let song_name = project
        .as_ref()
        .and_then(|p| p.song.as_ref())
        .map(|a| a.original_name.clone());
    let images = project
        .as_ref()
        .map(|p| {
            p.images
                .iter()
                .map(|image| (image.number, image.asset.original_name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let image_high_water = project
        .as_ref()
        .map(|p| {
            p.image_number_high_water
                .max(p.images.iter().map(|i| i.number).max().unwrap_or(0))
        })
        .unwrap_or(0);
    let renders = project.map(|project| project.renders).unwrap_or_default();
    Ok(view! {
        <div class="min-h-screen pb-12">
            <main class="mx-auto flex w-full max-w-[824px] flex-col gap-8 px-6 pt-8">
                page_intro()
                video_form(
                    title: title,
                    instructions: instructions,
                    credits: credits,
                    song_name: song_name,
                    images: images,
                    project_id: query.id.clone(),
                    image_high_water: image_high_water
                )
                previous_renders(
                    renders: renders,
                    project_id: query.id.clone().unwrap_or_default()
                )
            </main>
        </div>
    })
}

#[route(POST "./create-video")]
async fn submit_video(cx: &Cx, mut multipart: Multipart) -> Result<Json<serde_json::Value>> {
    let store = app_context::<Arc<store::Store>>(cx);
    let query = query_params::<HomeQuery>(cx)?;
    let old = match &query.id {
        Some(id) => Some(
            store
                .load_project(auth::username(cx), id)
                .await
                .map_err(|_| bad_request("Project not found"))?,
        ),
        None => None,
    };
    if let Some(id) = &query.id {
        if app_context::<renderer::Renderer>(cx)
            .status(id)
            .is_some_and(|p| p.state == "running")
        {
            return Err(bad_request("A render is already running").into());
        }
    }
    tracing::info!(
        project_id = query.id.as_deref().unwrap_or("new"),
        "Receiving project submission"
    );
    let pending = store.begin()?;
    let mut retained_numbers = Vec::new();
    let mut project = store::Project::default();
    let mut images = Vec::new();
    let mut image_numbers = Vec::new();
    while let Some(mut field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("").to_owned();
        match name.as_str() {
            "retained-image" => {
                let number: u64 = field
                    .text()
                    .await?
                    .parse()
                    .map_err(|_| bad_request("Invalid image number"))?;
                if retained_numbers.contains(&number) {
                    return Err(bad_request("Duplicate image number").into());
                }
                retained_numbers.push(number);
            }
            "song-title" => project.title = field.text().await?,
            "instructions" => project.instructions = field.text().await?,
            "credits-panel" => project.credits.push(field.text().await?),
            "image-number" => {
                let value = field.text().await?;
                let number: u64 = value
                    .parse()
                    .map_err(|_| bad_request("Invalid image number"))?;
                if number == 0 || image_numbers.contains(&number) {
                    return Err(bad_request("Image numbers must be positive and unique").into());
                }
                image_numbers.push(number);
            }
            "song-file" | "images" => {
                let Some(filename) = field
                    .file_name()
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                else {
                    continue;
                };
                if name == "song-file" && project.song.is_some() {
                    return Err(bad_request("Only one song file is allowed").into());
                }
                // Paths are generated by the server, never from an uploaded filename.
                let extension = std::path::Path::new(&filename)
                    .extension()
                    .and_then(|value| value.to_str())
                    .filter(|value| {
                        !value.is_empty()
                            && value.len() <= 10
                            && value.bytes().all(|b| b.is_ascii_alphanumeric())
                    })
                    .unwrap_or("bin")
                    .to_ascii_lowercase();
                let path = if name == "song-file" {
                    format!("song.{extension}")
                } else {
                    format!("image-{}.{extension}", images.len() + 1)
                };
                let mut output = tokio::fs::File::create(pending.path.join(&path)).await?;
                let mut size = 0u64;
                while let Some(chunk) = field.chunk().await? {
                    size += chunk.len() as u64;
                    output.write_all(&chunk).await?;
                }
                if size == 0 {
                    return Err(bad_request("Uploaded files must not be empty").into());
                }
                output.flush().await?;
                tracing::info!(asset_type = %name, size_bytes = size, "Upload saved");
                let path = if old.is_some() {
                    format!("uploads/{}/{path}", pending.id)
                } else {
                    path
                };
                let asset = store::Asset {
                    original_name: filename,
                    path,
                };
                if name == "song-file" {
                    project.song = Some(asset);
                } else {
                    images.push(asset);
                }
            }
            _ => {}
        }
    }
    if project.title.trim().is_empty() {
        return Err(bad_request("Song title is required").into());
    }
    if project.song.is_none() {
        project.song = old.as_ref().and_then(|p| p.song.clone());
    }
    let retained: Vec<store::Image> = retained_numbers
        .iter()
        .map(|number| {
            old.as_ref()
                .and_then(|p| p.images.iter().find(|i| i.number == *number))
                .cloned()
                .ok_or_else(|| bad_request("Unknown saved image"))
        })
        .collect::<std::result::Result<_, _>>()?;
    if image_numbers.iter().any(|n| retained_numbers.contains(n)) {
        return Err(bad_request("Image numbers must be unique").into());
    }
    if project.song.is_none() {
        return Err(bad_request("Song file is required").into());
    }
    if images.is_empty() && retained.is_empty() {
        return Err(bad_request("At least one image is required").into());
    }
    let high_water = old
        .as_ref()
        .map(|p| {
            p.image_number_high_water
                .max(p.images.iter().map(|i| i.number).max().unwrap_or(0))
        })
        .unwrap_or(0);
    if old.is_some() && image_numbers.iter().any(|number| *number <= high_water) {
        return Err(bad_request("New image numbers must increase").into());
    }
    project.image_number_high_water = image_numbers
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .max(high_water);
    if images.len() != image_numbers.len() {
        return Err(bad_request("Image files and image numbers must match").into());
    }
    project.images = retained;
    project.images.extend(
        image_numbers
            .into_iter()
            .zip(images)
            .map(|(number, asset)| store::Image { number, asset }),
    );
    let id = if let Some(id) = &query.id {
        store
            .update_project(auth::username(cx), id, pending, &mut project)
            .await?;
        id.clone()
    } else {
        store.commit(pending, auth::username(cx), &project).await?
    };
    tracing::info!(project_id = %id, image_count = project.images.len(), credit_count = project.credits.len(), "Project saved; starting render");
    app_context::<renderer::Renderer>(cx).start(
        id.clone(),
        auth::username(cx).to_owned(),
        store.project_path(&id),
        project,
    );
    Ok(Json(serde_json::json!({"id": id})))
}

#[route(GET "./render-status")]
async fn render_status(cx: &Cx) -> Result<Json<renderer::Progress>> {
    let query = query_params::<production::RenderQuery>(cx)?;
    app_context::<Arc<store::Store>>(cx)
        .load_project(auth::username(cx), &query.id)
        .await
        .map_err(|_| bad_request("Project not found"))?;
    tracing::debug!(project_id = %query.id, "Checking render status");
    let status = app_context::<renderer::Renderer>(cx)
        .status(&query.id)
        .ok_or_else(|| bad_request("Unknown or interrupted render"))?;
    Ok(Json(status))
}

#[derive(serde::Deserialize)]
struct RetryRequest {
    id: String,
}

#[route(POST "./cancel-render")]
async fn cancel_render(
    cx: &Cx,
    Json(request): Json<RetryRequest>,
) -> Result<Json<renderer::Progress>> {
    app_context::<Arc<store::Store>>(cx)
        .load_project(auth::username(cx), &request.id)
        .await
        .map_err(|_| bad_request("Project not found"))?;
    let renderer = app_context::<renderer::Renderer>(cx);
    tracing::info!(project_id = %request.id, "Render cancellation requested");
    if !renderer.cancel(&request.id) {
        return Err(bad_request("No active render to cancel").into());
    }
    Ok(Json(
        renderer.status(&request.id).expect("cancelled job exists"),
    ))
}

#[route(POST "./retry-render")]
async fn retry_render(
    cx: &Cx,
    Json(request): Json<RetryRequest>,
) -> Result<Json<serde_json::Value>> {
    let store = app_context::<Arc<store::Store>>(cx);
    tracing::info!(project_id = %request.id, "Render retry requested");
    let project = store
        .load_project(auth::username(cx), &request.id)
        .await
        .map_err(|_| bad_request("Project not found"))?;
    if !app_context::<renderer::Renderer>(cx).start(
        request.id.clone(),
        auth::username(cx).to_owned(),
        store.project_path(&request.id),
        project,
    ) {
        return Err(bad_request("A render is already running").into());
    }
    Ok(Json(serde_json::json!({"id": request.id})))
}

#[component]
async fn page_intro() -> Result<impl View> {
    Ok(view! {
        <section class="flex flex-col gap-[10px] sm:px-7" aria-labelledby="page-title">
            <p class="text-xs font-bold text-accent-ink">"CREATE A NEW VIDEO"</p>
            <h1
                id="page-title"
                class="text-[30px] leading-[1.2] font-bold sm:text-[34px]"
            >
                "Bring your song to life"
            </h1>
            <p class="text-base text-muted-foreground">
                "Add your music, photos, and a few directions. Flicksy takes it from there."
            </p>
        </section>
    })
}

#[component]
async fn video_form(
    title: String,
    instructions: String,
    credits: Vec<String>,
    song_name: Option<String>,
    images: Vec<(u64, String)>,
    project_id: Option<String>,
    image_high_water: u64,
) -> Result<impl View> {
    let action = project_id
        .clone()
        .map(|id| format!("/create-video?id={id}"))
        .unwrap_or_else(|| "/create-video".to_owned());
    Ok(view! {
        <form
            id="video-form"
            method="post"
            action=(action)
            enctype="multipart/form-data"
            class="flex min-w-0 flex-col gap-6 rounded-[20px] bg-card px-5 py-9 sm:px-10"
            aria-label="Create a video"
        >
            song_title_field(title: title)
            song_file_field(song_name: song_name)
            images_field(
                images: images,
                project_id: project_id.unwrap_or_default(),
                image_high_water: image_high_water
            )
            opening_credits_field(credits: credits)
            instructions_field(instructions: instructions)
            create_video_button()
            <div
                id="submission-status"
                role="status"
                aria-live="polite"
                class="text-sm text-muted-foreground"
            ></div>
        </form>
    })
}

#[component]
async fn previous_renders(renders: Vec<store::Render>, project_id: String) -> Result<impl View> {
    Ok(view! {
        if !renders.is_empty() {
            <section
                class="flex min-w-0 flex-col gap-3"
                aria-labelledby="renders-title"
            >
                <h2 id="renders-title" class="text-[25px] leading-[1.2] font-bold">
                    "Previous renders"
                </h2>
                <p class="text-sm text-muted-foreground">
                    "Your earlier versions stay available to preview or download."
                </p>
                <div class="flex flex-col divide-y divide-border">
                    for render in renders {
                        <article
                            class="flex min-w-0 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:gap-4"
                            aria-label=(format!("Render {}", render.number))
                        >
                            <div class="flex min-w-0 flex-1 items-center gap-4">
                                <div
                                    class="flex h-[53px] w-[74px] shrink-0 items-center justify-center rounded-lg bg-surface-raised text-muted-foreground"
                                >
                                    "▶"
                                </div>
                                <div class="flex min-w-0 flex-1 flex-col gap-1">
                                    <h3 class="text-[15px] font-semibold">
                                        (format!("Render {}", render.number))
                                    </h3>
                                </div>
                            </div>
                            <div class="flex shrink-0 items-center gap-3">
                                <a
                                    href=(format!(
                                        "/preview?id={}&render={}",
                                        project_id,
                                        render.number,
                                    ))
                                    aria-label=(format!("Preview render {}", render.number))
                                    class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-accent-soft text-[13px] font-semibold text-accent-ink cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-accent-hover active:bg-accent-hover"
                                >
                                    "Preview"
                                </a>
                                <a
                                    href=(format!(
                                        "/preview/video?id={}&render={}",
                                        project_id,
                                        render.number,
                                    ))
                                    download="download"
                                    aria-label=(format!("Download render {}", render.number))
                                    class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-border bg-card text-[13px] font-semibold cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:border-accent-border hover:bg-accent-soft active:bg-accent-hover"
                                >
                                    "↓ Download"
                                </a>
                            </div>
                        </article>
                    }
                </div>
            </section>
        }
    })
}

#[component]
async fn song_title_field(title: String) -> Result<impl View> {
    Ok(view! {
        <div class="flex min-w-0 flex-col gap-3">
            <label for="song-title" class="text-[15px] font-semibold">
                "Song title"
            </label>
            <input
                id="song-title"
                name="song-title"
                value=(title)
                type="text"
                required="required"
                placeholder="e.g. A New Beginning"
                class="h-[54px] w-full min-w-0 rounded-[11px] border border-border bg-card px-4 text-[15px] placeholder:text-placeholder focus:outline-2 focus:outline-[#6554e8]"
            >
        </div>
    })
}

#[component]
async fn song_file_field(song_name: Option<String>) -> Result<impl View> {
    Ok(view! {
        <div class="flex min-w-0 flex-col gap-3">
            <h2 class="text-[15px] font-semibold">"Song file"</h2>
            <label
                id="song-dropzone"
                for="song-file"
                class="flex min-w-0 cursor-pointer items-center gap-5 rounded-xl border border-border bg-surface px-[19px] py-6 transition hover:border-[#6554e8] hover:bg-accent-soft focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-[#6554e8] motion-reduce:transition-none"
            >
                <input
                    id="song-file"
                    name="song-file"
                    type="file"
                    required=(song_name.is_none())
                    accept="audio/*,video/*"
                    class="sr-only"
                    aria-describedby="song-file-hint song-file-status"
                >
                <span
                    class="flex size-[58px] shrink-0 items-center justify-center rounded-xl bg-accent-soft text-3xl text-accent-ink"
                    aria-hidden="true"
                >
                    "♫"
                </span>
                <span class="flex min-w-0 flex-1 flex-col gap-2">
                    <span id="song-file-name" class="break-all text-base font-semibold">
                        (song_name.as_deref().unwrap_or("Choose an audio or video file"))
                    </span>
                    <span id="song-file-hint" class="text-[13px] text-muted-foreground">
                        "or drag and drop · audio or video"
                    </span>
                    <span class="text-xs text-muted-foreground">
                        "For video files, only the audio track will be used."
                    </span>
                    <span
                        id="song-file-status"
                        class="text-[13px] text-danger-ink"
                        role="status"
                        aria-live="polite"
                    ></span>
                </span>
            </label>
        </div>
    })
}

#[component]
async fn images_field(
    images: Vec<(u64, String)>,
    project_id: String,
    image_high_water: u64,
) -> Result<impl View> {
    Ok(view! {
        <div class="flex min-w-0 flex-col gap-3">
            <h2 class="text-[15px] font-semibold">"Photos and images"</h2>
            <label
                id="images-dropzone"
                for="images"
                class="flex min-w-0 cursor-pointer items-center gap-5 rounded-xl border border-border bg-surface px-[19px] py-6 transition hover:border-[#6554e8] hover:bg-accent-soft focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-[#6554e8] motion-reduce:transition-none"
            >
                <input
                    id="images"
                    name="images"
                    type="file"
                    accept=".jpg,.jpeg,.png,.webp,image/jpeg,image/png,image/webp"
                    required=(images.is_empty())
                    multiple="multiple"
                    class="sr-only"
                    aria-describedby="images-hint images-status"
                >
                <span
                    class="flex size-[58px] shrink-0 items-center justify-center rounded-xl bg-accent-soft text-3xl text-accent-ink"
                    aria-hidden="true"
                >
                    "▧"
                </span>
                <span class="flex min-w-0 flex-1 flex-col gap-2">
                    <span class="text-base font-semibold">"Choose images"</span>
                    <span id="images-hint" class="text-[13px] text-muted-foreground">
                        "or drag and drop · JPG, PNG, WebP"
                    </span>
                    <span class="text-xs text-muted-foreground">
                        "Add as many images as you like"
                    </span>
                </span>
            </label>
            <p
                id="images-status"
                class="text-[13px] text-danger-ink"
                role="status"
                aria-live="polite"
            ></p>
            <ul
                id="images-list"
                data-image-high-water=(image_high_water)
                class="image-grid"
                aria-label="Selected images"
                aria-live="polite"
            >
                for (number, name) in images {
                    <li class="image-tile" data-saved-number=(number)>
                        <img
                            src=(format!(
                                "/preview/image?id={}&number={}",
                                project_id,
                                number,
                            ))
                            alt=""
                        >
                        <span class="image-tile-number">
                            (format!("Image {number}"))
                        </span>
                        <span class="image-tile-name">(name.as_str())</span>
                        <button
                            type="button"
                            class="image-tile-remove"
                            aria-label=(format!("Remove {name}"))
                        >
                            "×"
                        </button>
                        <input type="hidden" name="retained-image" value=(number)>
                    </li>
                }
            </ul>
        </div>
    })
}

#[component]
async fn opening_credits_field(mut credits: Vec<String>) -> Result<impl View> {
    if credits.is_empty() {
        credits.push(String::new());
    }
    Ok(view! {
        <div class="flex min-w-0 flex-col gap-3">
            <div class="flex flex-wrap items-center gap-x-4 gap-y-1">
                <h2 class="text-[15px] font-semibold">"Opening credits"</h2>
                <span class="text-xs text-muted-foreground">"Optional"</span>
            </div>
            <p class="text-[13px] text-muted-foreground">
                "Each panel becomes a separate opening screen. Leave blank to skip credits."
            </p>
            <div id="credits-panels" class="flex min-w-0 flex-col gap-3">
                for (index, text) in credits.into_iter().enumerate() {
                    <div
                        class="credits-panel flex min-w-0 flex-col gap-[9px] rounded-[10px] bg-accent-soft p-4"
                    >
                        <div class="credits-panel-heading">
                            <label
                                for=(format!("credits-panel-{}", index + 1))
                                class="text-[13px] font-semibold"
                            >
                                (format!("Panel {}", index + 1))
                            </label>
                            <button
                                type="button"
                                class="credits-panel-remove"
                                aria-label=(format!("Remove panel {}", index + 1))
                            >
                                "×"
                            </button>
                        </div>
                        <textarea
                            id=(format!("credits-panel-{}", index + 1))
                            name="credits-panel"
                            rows="3"
                            placeholder="Type the text to appear on this screen…"
                            class="min-h-[105px] w-full min-w-0 rounded-[11px] border border-border bg-card p-4 text-sm placeholder:text-placeholder focus:outline-2 focus:outline-[#6554e8]"
                        >
                            (text)
                        </textarea>
                    </div>
                }
            </div>
            <button
                id="add-credits-panel"
                type="button"
                class="min-h-[43px] w-full rounded-[10px] border border-accent-border bg-accent-soft px-4 py-3 text-sm font-semibold text-accent-ink cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-accent-hover active:bg-accent-hover"
            >
                "+ Add another panel"
            </button>
        </div>
    })
}

#[component]
async fn instructions_field(instructions: String) -> Result<impl View> {
    Ok(view! {
        <div class="flex min-w-0 flex-col gap-3">
            <div class="flex flex-wrap items-center gap-x-4 gap-y-1">
                <label for="instructions" class="text-[15px] font-semibold">
                    "Additional instructions"
                </label>
                <span class="text-xs text-muted-foreground">"Optional"</span>
            </div>
            <textarea
                id="instructions"
                name="instructions"
                rows="3"
                placeholder="Describe the mood, opening credits, image order, transitions…"
                class="min-h-[104px] w-full min-w-0 rounded-[11px] border border-border bg-card p-4 text-sm placeholder:text-placeholder focus:outline-2 focus:outline-[#6554e8]"
            >
                (instructions)
            </textarea>
        </div>
    })
}

#[component]
async fn create_video_button() -> Result<impl View> {
    Ok(view! {
        <button
            type="submit"
            class="flex min-h-14 w-full items-center justify-center rounded-xl bg-[#6554e8] px-4 py-4 text-[17px] font-bold text-white cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#5544d4] active:bg-[#4939bd]"
        >
            "✦ Create video"
        </button>
    })
}

fn read_password() -> std::io::Result<String> {
    use std::io::{BufRead, IsTerminal, Write};
    let stdin = std::io::stdin();
    let terminal = stdin.is_terminal();
    if terminal {
        eprint!("Password: ");
        std::io::stderr().flush()?;
        if !std::process::Command::new("stty")
            .arg("-echo")
            .status()?
            .success()
        {
            return Err(std::io::Error::other("Could not disable terminal echo"));
        }
    }
    let mut password = String::new();
    let result = stdin.lock().read_line(&mut password);
    if terminal {
        let restored = std::process::Command::new("stty").arg("echo").status();
        eprintln!();
        if !restored?.success() {
            return Err(std::io::Error::other("Could not restore terminal echo"));
        }
    }
    result?;
    Ok(password.trim_end_matches(['\r', '\n']).to_owned())
}
