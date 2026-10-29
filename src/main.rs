use clap::Parser;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use topcoat::{
    Result,
    asset::{AssetBundle, RouterBuilderAssetExt},
    context::{Cx, app_context},
    font::fontsource::fontsource_font,
    router::{
        BodyLimit, RouterBuilderDiscoverExt, Slot, content::multipart::Multipart,
        error::bad_request, layout, module_router, page, route,
    },
    tailwind,
    view::{View, component, view},
};

mod config;
mod store;
pub mod types;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::load(&Args::parse().config)?;
    let store = store::Store::open(config.projects_dir)?;
    Ok(topcoat::start(
        module_router!()
            .discover()
            .app_context(store)
            .layer(BodyLimit::max(1024 * 1024 * 1024).at("/create-video"))
            .assets(AssetBundle::load()?)
            .build(),
    )
    .await?)
}

#[layout]
async fn layout(slot: Slot<'_>) -> Result<impl View> {
    Ok(view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                topcoat::font::link(font: fontsource_font!(GEIST))
                <link rel="stylesheet" href=(tailwind::stylesheet!())>
                <title>"Flicksy"</title>
                <script src=(topcoat::asset::asset!("song-file.js")) defer="defer"></script>
                <script src=(topcoat::asset::asset!("images.js")) defer="defer"></script>
                <script src=(topcoat::asset::asset!("credits.js")) defer="defer"></script>
                <script
                    src=(topcoat::asset::asset!("form-validation.js"))
                    defer="defer"
                ></script>
                topcoat::dev::script()
            </head>
            <body class="m-0 bg-[#f7f8fc] text-[#19213a]">
                <header class="bg-white px-6 py-[26px]">
                    <div
                        class="mx-auto flex w-full max-w-[1192px] flex-wrap items-center gap-3"
                    >
                        <div class="flex shrink-0 items-center gap-3">
                            <div
                                class="flex size-9 items-center justify-center rounded-[11px] bg-[#6554e8] text-[23px] font-bold text-white"
                            >
                                "✦"
                            </div>
                            <span class="text-[26px] leading-[1.2] font-bold">
                                "flicksy"
                            </span>
                        </div>
                        <p class="text-[13px] text-[#6b7488]">
                            "Turn songs and photos into videos."
                        </p>
                    </div>
                </header>
                (slot)
            </body>
        </html>
    })
}

#[page]
async fn home() -> Result<impl View> {
    let renders: Vec<store::Render> = Vec::new();
    Ok(view! {
        <div class="min-h-screen pb-12">
            <main class="mx-auto flex w-full max-w-[824px] flex-col gap-8 px-6 pt-8">
                page_intro()
                video_form()
                previous_renders(renders: renders)
            </main>
        </div>
    })
}

#[route(POST "./create-video")]
async fn submit_video(cx: &Cx, mut multipart: Multipart) -> Result<String> {
    let store = app_context::<store::Store>(cx);
    let pending = store.begin()?;
    let mut project = store::Project::default();
    let mut images = Vec::new();
    let mut image_numbers = Vec::new();
    while let Some(mut field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("").to_owned();
        match name.as_str() {
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
        return Err(bad_request("Song file is required").into());
    }
    if images.is_empty() {
        return Err(bad_request("At least one image is required").into());
    }
    if images.len() != image_numbers.len() {
        return Err(bad_request("Image files and image numbers must match").into());
    }
    project.images = image_numbers
        .into_iter()
        .zip(images)
        .map(|(number, asset)| store::Image { number, asset })
        .collect();
    let id = store.commit(pending, &project).await?;
    Ok(format!("Project {id} saved."))
}

#[component]
async fn page_intro() -> Result<impl View> {
    Ok(view! {
        <section class="flex flex-col gap-[10px] sm:px-7" aria-labelledby="page-title">
            <p class="text-xs font-bold text-[#6554e8]">"CREATE A NEW VIDEO"</p>
            <h1
                id="page-title"
                class="text-[30px] leading-[1.2] font-bold sm:text-[34px]"
            >
                "Bring your song to life"
            </h1>
            <p class="text-base text-[#6b7488]">
                "Add your music, photos, and a few directions. Flicksy takes it from there."
            </p>
        </section>
    })
}

#[component]
async fn video_form() -> Result<impl View> {
    Ok(view! {
        <form
            method="post"
            action="/create-video"
            enctype="multipart/form-data"
            class="flex min-w-0 flex-col gap-6 rounded-[20px] bg-white px-5 py-9 sm:px-10"
            aria-label="Create a video"
        >
            song_title_field()
            song_file_field()
            images_field()
            opening_credits_field()
            instructions_field()
            create_video_button()
        </form>
    })
}

#[component]
async fn previous_renders(renders: Vec<store::Render>) -> Result<impl View> {
    Ok(view! {
        if !renders.is_empty() {
            <section
                class="flex min-w-0 flex-col gap-3"
                aria-labelledby="renders-title"
            >
                <h2 id="renders-title" class="text-[25px] leading-[1.2] font-bold">
                    "Previous renders"
                </h2>
                <p class="text-sm text-[#6b7488]">
                    "Your earlier versions stay available to preview or download."
                </p>
                <div class="flex flex-col divide-y divide-[#dde2ec]">
                    for render in renders {
                        <article
                            class="flex min-w-0 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:gap-4"
                            aria-label=(format!("Render {}", render.number))
                        >
                            <div class="flex min-w-0 flex-1 items-center gap-4">
                                <div
                                    class="flex h-[53px] w-[74px] shrink-0 items-center justify-center rounded-lg bg-[#e6e9f1] text-[#6b7488]"
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
                                <button
                                    type="button"
                                    aria-label=(format!("Preview render {}", render.number))
                                    class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8] cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#e5e1ff] active:bg-[#d9d3ff]"
                                >
                                    "Preview"
                                </button>
                                <button
                                    type="button"
                                    aria-label=(format!("Download render {}", render.number))
                                    class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:border-[#bcb4f5] hover:bg-[#f7f6ff] active:bg-[#ece9ff]"
                                >
                                    "↓ Download"
                                </button>
                            </div>
                        </article>
                    }
                </div>
            </section>
        }
    })
}

#[component]
async fn song_title_field() -> Result<impl View> {
    Ok(view! {
        <div class="flex min-w-0 flex-col gap-3">
            <label for="song-title" class="text-[15px] font-semibold">
                "Song title"
            </label>
            <input
                id="song-title"
                name="song-title"
                type="text"
                required="required"
                placeholder="e.g. A New Beginning"
                class="h-[54px] w-full min-w-0 rounded-[11px] border border-[#dde2ec] bg-white px-4 text-[15px] placeholder:text-[#949baa] focus:outline-2 focus:outline-[#6554e8]"
            >
        </div>
    })
}

#[component]
async fn song_file_field() -> Result<impl View> {
    Ok(view! {
        <div class="flex min-w-0 flex-col gap-3">
            <h2 class="text-[15px] font-semibold">"Song file"</h2>
            <label
                id="song-dropzone"
                for="song-file"
                class="flex min-w-0 cursor-pointer items-center gap-5 rounded-xl border border-[#dde2ec] bg-[#fafbff] px-[19px] py-6 transition hover:border-[#6554e8] hover:bg-[#f2f0ff] focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-[#6554e8] motion-reduce:transition-none"
            >
                <input
                    id="song-file"
                    name="song-file"
                    type="file"
                    required="required"
                    accept="audio/*,video/*"
                    class="sr-only"
                    aria-describedby="song-file-hint song-file-status"
                >
                <span
                    class="flex size-[58px] shrink-0 items-center justify-center rounded-xl bg-[#f2f0ff] text-3xl text-[#6554e8]"
                    aria-hidden="true"
                >
                    "♫"
                </span>
                <span class="flex min-w-0 flex-1 flex-col gap-2">
                    <span id="song-file-name" class="break-all text-base font-semibold">
                        "Choose an audio or video file"
                    </span>
                    <span id="song-file-hint" class="text-[13px] text-[#6b7488]">
                        "or drag and drop · audio or video"
                    </span>
                    <span class="text-xs text-[#6b7488]">
                        "For video files, only the audio track will be used."
                    </span>
                    <span
                        id="song-file-status"
                        class="text-[13px] text-[#a12c44]"
                        role="status"
                        aria-live="polite"
                    ></span>
                </span>
            </label>
        </div>
    })
}

#[component]
async fn images_field() -> Result<impl View> {
    Ok(view! {
        <div class="flex min-w-0 flex-col gap-3">
            <h2 class="text-[15px] font-semibold">"Photos and images"</h2>
            <label
                id="images-dropzone"
                for="images"
                class="flex min-w-0 cursor-pointer items-center gap-5 rounded-xl border border-[#dde2ec] bg-[#fafbff] px-[19px] py-6 transition hover:border-[#6554e8] hover:bg-[#f2f0ff] focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-[#6554e8] motion-reduce:transition-none"
            >
                <input
                    id="images"
                    name="images"
                    type="file"
                    accept=".jpg,.jpeg,.png,.webp,image/jpeg,image/png,image/webp"
                    required="required"
                    multiple="multiple"
                    class="sr-only"
                    aria-describedby="images-hint images-status"
                >
                <span
                    class="flex size-[58px] shrink-0 items-center justify-center rounded-xl bg-[#f2f0ff] text-3xl text-[#6554e8]"
                    aria-hidden="true"
                >
                    "▧"
                </span>
                <span class="flex min-w-0 flex-1 flex-col gap-2">
                    <span class="text-base font-semibold">"Choose images"</span>
                    <span id="images-hint" class="text-[13px] text-[#6b7488]">
                        "or drag and drop · JPG, PNG, WebP"
                    </span>
                    <span class="text-xs text-[#6b7488]">
                        "Add as many images as you like"
                    </span>
                </span>
            </label>
            <p
                id="images-status"
                class="text-[13px] text-[#a12c44]"
                role="status"
                aria-live="polite"
            ></p>
            <ul
                id="images-list"
                class="image-grid"
                aria-label="Selected images"
                aria-live="polite"
            ></ul>
        </div>
    })
}

#[component]
async fn opening_credits_field() -> Result<impl View> {
    Ok(view! {
        <div class="flex min-w-0 flex-col gap-3">
            <div class="flex flex-wrap items-center gap-x-4 gap-y-1">
                <h2 class="text-[15px] font-semibold">"Opening credits"</h2>
                <span class="text-xs text-[#6b7488]">"Optional"</span>
            </div>
            <p class="text-[13px] text-[#6b7488]">
                "Each panel becomes a separate opening screen. Leave blank to skip credits."
            </p>
            <div id="credits-panels" class="flex min-w-0 flex-col gap-3">
                <div
                    class="credits-panel flex min-w-0 flex-col gap-[9px] rounded-[10px] bg-[#f7f6ff] p-4"
                >
                    <div class="credits-panel-heading">
                        <label for="credits-panel-1" class="text-[13px] font-semibold">
                            "Panel 1"
                        </label>
                        <button
                            type="button"
                            class="credits-panel-remove"
                            aria-label="Remove panel 1"
                            hidden="hidden"
                        >
                            "×"
                        </button>
                    </div>
                    <textarea
                        id="credits-panel-1"
                        name="credits-panel"
                        rows="3"
                        placeholder="Type the text to appear on this screen…"
                        class="min-h-[105px] w-full min-w-0 rounded-[11px] border border-[#dde2ec] bg-white p-4 text-sm placeholder:text-[#949baa] focus:outline-2 focus:outline-[#6554e8]"
                    ></textarea>
                </div>
            </div>
            <button
                id="add-credits-panel"
                type="button"
                class="min-h-[43px] w-full rounded-[10px] border border-[#d9d3ff] bg-[#f7f6ff] px-4 py-3 text-sm font-semibold text-[#6554e8] cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#ece9ff] active:bg-[#e1dcff]"
            >
                "+ Add another panel"
            </button>
        </div>
    })
}

#[component]
async fn instructions_field() -> Result<impl View> {
    Ok(view! {
        <div class="flex min-w-0 flex-col gap-3">
            <div class="flex flex-wrap items-center gap-x-4 gap-y-1">
                <label for="instructions" class="text-[15px] font-semibold">
                    "Additional instructions"
                </label>
                <span class="text-xs text-[#6b7488]">"Optional"</span>
            </div>
            <textarea
                id="instructions"
                name="instructions"
                rows="3"
                placeholder="Describe the mood, opening credits, image order, transitions…"
                class="min-h-[104px] w-full min-w-0 rounded-[11px] border border-[#dde2ec] bg-white p-4 text-sm placeholder:text-[#949baa] focus:outline-2 focus:outline-[#6554e8]"
            ></textarea>
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
