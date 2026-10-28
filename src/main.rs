use topcoat::{
    Result,
    asset::{AssetBundle, RouterBuilderAssetExt},
    font::fontsource::fontsource_font,
    router::{RouterBuilderDiscoverExt, Slot, layout, module_router, page},
    tailwind,
    view::{View, view},
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
                <section
                    class="flex flex-col gap-[10px] sm:px-7"
                    aria-labelledby="page-title"
                >
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
                <section
                    class="flex min-w-0 flex-col gap-6 rounded-[20px] bg-white px-5 py-9 sm:px-10"
                    aria-label="Create a video"
                >
                    <div class="flex min-w-0 flex-col gap-3">
                        <label for="song-title" class="text-[15px] font-semibold">
                            "Song title"
                        </label>
                        <input
                            id="song-title"
                            type="text"
                            placeholder="e.g. A New Beginning"
                            class="h-[54px] w-full min-w-0 rounded-[11px] border border-[#dde2ec] bg-white px-4 text-[15px] placeholder:text-[#949baa] focus:outline-2 focus:outline-[#6554e8]"
                        >
                    </div>
                    <div class="flex min-w-0 flex-col gap-3">
                        <h2 class="text-[15px] font-semibold">"Song file"</h2>
                        <div
                            class="flex min-w-0 items-center gap-5 rounded-xl border border-[#dde2ec] bg-[#fafbff] px-[19px] py-6"
                        >
                            <div
                                class="flex size-[58px] shrink-0 items-center justify-center rounded-xl bg-[#f2f0ff] text-3xl text-[#6554e8]"
                            >
                                "♫"
                            </div>
                            <div class="flex min-w-0 flex-1 flex-col gap-2">
                                <p class="text-base font-semibold">
                                    "Choose an audio file"
                                </p>
                                <p class="text-[13px] text-[#6b7488]">
                                    "or drag and drop · MP3, WAV, M4A"
                                </p>
                            </div>
                        </div>
                    </div>
                    <div class="flex min-w-0 flex-col gap-3">
                        <h2 class="text-[15px] font-semibold">"Photos and images"</h2>
                        <div
                            class="flex min-w-0 items-center gap-5 rounded-xl border border-[#dde2ec] bg-[#fafbff] px-[19px] py-6"
                        >
                            <div
                                class="flex size-[58px] shrink-0 items-center justify-center rounded-xl bg-[#f2f0ff] text-3xl text-[#6554e8]"
                            >
                                "▧"
                            </div>
                            <div class="flex min-w-0 flex-1 flex-col gap-2">
                                <p class="text-base font-semibold">"Choose images"</p>
                                <p class="text-[13px] text-[#6b7488]">
                                    "or drag and drop · JPG, PNG, WebP"
                                </p>
                                <p class="text-xs text-[#6b7488]">
                                    "Add as many images as you like"
                                </p>
                            </div>
                        </div>
                    </div>
                    <div class="flex min-w-0 flex-col gap-3">
                        <div class="flex flex-wrap items-center gap-x-4 gap-y-1">
                            <h2 class="text-[15px] font-semibold">"Opening credits"</h2>
                            <span class="text-xs text-[#6b7488]">"Optional"</span>
                        </div>
                        <p class="text-[13px] text-[#6b7488]">
                            "Each panel becomes a separate opening screen. Leave blank to skip credits."
                        </p>
                        <div class="flex min-w-0 flex-col gap-[9px]">
                            <label
                                for="credits-panel"
                                class="text-[13px] font-semibold"
                            >
                                "Panel 1"
                            </label>
                            <textarea
                                id="credits-panel"
                                rows="3"
                                placeholder="Type the text to appear on this screen…"
                                class="min-h-[105px] w-full min-w-0 rounded-[11px] border border-[#dde2ec] bg-white p-4 text-sm placeholder:text-[#949baa] focus:outline-2 focus:outline-[#6554e8]"
                            ></textarea>
                        </div>
                        <button
                            type="button"
                            class="min-h-[43px] w-full rounded-[10px] border border-[#d9d3ff] bg-[#f7f6ff] px-4 py-3 text-sm font-semibold text-[#6554e8]"
                        >
                            "+ Add another panel"
                        </button>
                    </div>
                    <div class="flex min-w-0 flex-col gap-3">
                        <div class="flex flex-wrap items-center gap-x-4 gap-y-1">
                            <label for="instructions" class="text-[15px] font-semibold">
                                "Additional instructions"
                            </label>
                            <span class="text-xs text-[#6b7488]">"Optional"</span>
                        </div>
                        <textarea
                            id="instructions"
                            rows="3"
                            placeholder="Describe the mood, opening credits, image order, transitions…"
                            class="min-h-[104px] w-full min-w-0 rounded-[11px] border border-[#dde2ec] bg-white p-4 text-sm placeholder:text-[#949baa] focus:outline-2 focus:outline-[#6554e8]"
                        ></textarea>
                    </div>
                    <button
                        type="button"
                        class="flex min-h-14 w-full items-center justify-center rounded-xl bg-[#6554e8] px-4 py-4 text-[17px] font-bold text-white"
                    >
                        "✦ Create video"
                    </button>
                </section>
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
                                    class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8]"
                                >
                                    "Preview"
                                </button>
                                <button
                                    type="button"
                                    aria-label="Download render 5"
                                    class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold"
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
                                    class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8]"
                                >
                                    "Preview"
                                </button>
                                <button
                                    type="button"
                                    aria-label="Download render 4"
                                    class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold"
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
                                    class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8]"
                                >
                                    "Preview"
                                </button>
                                <button
                                    type="button"
                                    aria-label="Download render 3"
                                    class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold"
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
                                    class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8]"
                                >
                                    "Preview"
                                </button>
                                <button
                                    type="button"
                                    aria-label="Download render 2"
                                    class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold"
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
                                    class="flex h-9 w-[102px] items-center justify-center rounded-lg bg-[#f2f0ff] text-[13px] font-semibold text-[#6554e8]"
                                >
                                    "Preview"
                                </button>
                                <button
                                    type="button"
                                    aria-label="Download render 1"
                                    class="flex h-9 w-[139px] items-center justify-center rounded-lg border border-[#dde2ec] bg-white text-[13px] font-semibold"
                                >
                                    "↓ Download"
                                </button>
                            </div>
                        </article>
                    </div>
                </section>
            </main>
        </div>
    })
}
