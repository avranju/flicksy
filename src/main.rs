use topcoat::{
    Result,
    asset::{AssetBundle, RouterBuilderAssetExt},
    font::fontsource::fontsource_font,
    router::{RouterBuilderDiscoverExt, Slot, StatusCode, layout, module_router, page, route},
    tailwind,
    view::{View, component, view},
};

pub mod types;

#[tokio::main]
async fn main() -> Result<()> {
    Ok(topcoat::start(
        module_router!()
            .discover()
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
    Ok(view! {
        <div class="min-h-screen pb-12">
            <main class="mx-auto flex w-full max-w-[824px] flex-col gap-8 px-6 pt-8">
                page_intro()
                video_form()
                previous_renders()
            </main>
        </div>
    })
}

#[route(POST "./create-video")]
async fn submit_video() -> Result<StatusCode> {
    Ok(StatusCode::NOT_IMPLEMENTED)
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
async fn previous_renders() -> Result<impl View> {
    Ok(view! {
        <section class="flex min-w-0 flex-col gap-3" aria-labelledby="renders-title">
            <h2 id="renders-title" class="text-[25px] leading-[1.2] font-bold">
                "Previous renders"
            </h2>
            <p class="text-sm text-[#6b7488]">
                "Your earlier versions stay available to preview or download."
            </p>
            <div class="flex flex-col divide-y divide-[#dde2ec]">
                <article
                    class="flex min-w-0 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:gap-4"
                    aria-label="Render 5"
                >
                    <div class="flex min-w-0 flex-1 items-center gap-4">
                        <div
                            class="flex h-[53px] w-[74px] shrink-0 items-center justify-center rounded-lg bg-[#dcd6ff] text-[#6554e8]"
                        >
                            "▶"
                        </div>
                        <div class="flex min-w-0 flex-1 flex-col gap-1">
                            <h3 class="text-[15px] font-semibold">
                                "Render 5 · Latest"
                            </h3>
                            <p class="text-xs text-[#6b7488]">"Current version"</p>
                        </div>
                    </div>
                    <div class="flex shrink-0 items-center gap-3">
                        <button
                            type="button"
                            aria-label="Preview render 5"
                            class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8] cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#e5e1ff] active:bg-[#d9d3ff]"
                        >
                            "Preview"
                        </button>
                        <button
                            type="button"
                            aria-label="Download render 5"
                            class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:border-[#bcb4f5] hover:bg-[#f7f6ff] active:bg-[#ece9ff]"
                        >
                            "↓ Download"
                        </button>
                    </div>
                </article>
                <article
                    class="flex min-w-0 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:gap-4"
                    aria-label="Render 4"
                >
                    <div class="flex min-w-0 flex-1 items-center gap-4">
                        <div
                            class="flex h-[53px] w-[74px] shrink-0 items-center justify-center rounded-lg bg-[#e6e9f1] text-[#6b7488]"
                        >
                            "▶"
                        </div>
                        <div class="flex min-w-0 flex-1 flex-col gap-1">
                            <h3 class="text-[15px] font-semibold">"Render 4"</h3>
                            <p class="text-xs text-[#6b7488]">"Earlier version"</p>
                        </div>
                    </div>
                    <div class="flex shrink-0 items-center gap-3">
                        <button
                            type="button"
                            aria-label="Preview render 4"
                            class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8] cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#e5e1ff] active:bg-[#d9d3ff]"
                        >
                            "Preview"
                        </button>
                        <button
                            type="button"
                            aria-label="Download render 4"
                            class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:border-[#bcb4f5] hover:bg-[#f7f6ff] active:bg-[#ece9ff]"
                        >
                            "↓ Download"
                        </button>
                    </div>
                </article>
                <article
                    class="flex min-w-0 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:gap-4"
                    aria-label="Render 3"
                >
                    <div class="flex min-w-0 flex-1 items-center gap-4">
                        <div
                            class="flex h-[53px] w-[74px] shrink-0 items-center justify-center rounded-lg bg-[#e6e9f1] text-[#6b7488]"
                        >
                            "▶"
                        </div>
                        <div class="flex min-w-0 flex-1 flex-col gap-1">
                            <h3 class="text-[15px] font-semibold">"Render 3"</h3>
                            <p class="text-xs text-[#6b7488]">"Earlier version"</p>
                        </div>
                    </div>
                    <div class="flex shrink-0 items-center gap-3">
                        <button
                            type="button"
                            aria-label="Preview render 3"
                            class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8] cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#e5e1ff] active:bg-[#d9d3ff]"
                        >
                            "Preview"
                        </button>
                        <button
                            type="button"
                            aria-label="Download render 3"
                            class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:border-[#bcb4f5] hover:bg-[#f7f6ff] active:bg-[#ece9ff]"
                        >
                            "↓ Download"
                        </button>
                    </div>
                </article>
                <article
                    class="flex min-w-0 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:gap-4"
                    aria-label="Render 2"
                >
                    <div class="flex min-w-0 flex-1 items-center gap-4">
                        <div
                            class="flex h-[53px] w-[74px] shrink-0 items-center justify-center rounded-lg bg-[#e6e9f1] text-[#6b7488]"
                        >
                            "▶"
                        </div>
                        <div class="flex min-w-0 flex-1 flex-col gap-1">
                            <h3 class="text-[15px] font-semibold">"Render 2"</h3>
                            <p class="text-xs text-[#6b7488]">"Earlier version"</p>
                        </div>
                    </div>
                    <div class="flex shrink-0 items-center gap-3">
                        <button
                            type="button"
                            aria-label="Preview render 2"
                            class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8] cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#e5e1ff] active:bg-[#d9d3ff]"
                        >
                            "Preview"
                        </button>
                        <button
                            type="button"
                            aria-label="Download render 2"
                            class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:border-[#bcb4f5] hover:bg-[#f7f6ff] active:bg-[#ece9ff]"
                        >
                            "↓ Download"
                        </button>
                    </div>
                </article>
                <article
                    class="flex min-w-0 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:gap-4"
                    aria-label="Render 1"
                >
                    <div class="flex min-w-0 flex-1 items-center gap-4">
                        <div
                            class="flex h-[53px] w-[74px] shrink-0 items-center justify-center rounded-lg bg-[#e6e9f1] text-[#6b7488]"
                        >
                            "▶"
                        </div>
                        <div class="flex min-w-0 flex-1 flex-col gap-1">
                            <h3 class="text-[15px] font-semibold">"Render 1"</h3>
                            <p class="text-xs text-[#6b7488]">"Earlier version"</p>
                        </div>
                    </div>
                    <div class="flex shrink-0 items-center gap-3">
                        <button
                            type="button"
                            aria-label="Preview render 1"
                            class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8] cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#e5e1ff] active:bg-[#d9d3ff]"
                        >
                            "Preview"
                        </button>
                        <button
                            type="button"
                            aria-label="Download render 1"
                            class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:border-[#bcb4f5] hover:bg-[#f7f6ff] active:bg-[#ece9ff]"
                        >
                            "↓ Download"
                        </button>
                    </div>
                </article>
            </div>
        </section>
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
