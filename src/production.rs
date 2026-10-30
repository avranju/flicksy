use crate::store;
use std::sync::Arc;
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{error::bad_request, page, query_params},
    view::{View, view},
};

#[query_params(error = bad_request)]
pub(crate) struct RenderQuery {
    pub(crate) id: String,
}

#[page]
async fn production(cx: &Cx) -> Result<impl View> {
    let query = query_params::<RenderQuery>(cx)?;
    let project = app_context::<Arc<store::Store>>(cx)
        .load_project(&query.id)
        .await
        .map_err(|_| bad_request("Project not found"))?;
    Ok(view! {
        <main class="mx-auto w-full max-w-[824px] px-6 pt-11 pb-16">
            <section class="px-0 sm:px-7" aria-labelledby="production-title">
                <p class="text-xs font-bold text-accent-ink">"VIDEO PRODUCTION"</p>
                <h1
                    id="production-title"
                    class="mt-4 text-[30px] leading-[1.2] font-bold sm:text-[34px]"
                >
                    "Making your video"
                </h1>
                <p class="mt-4 text-base text-muted-foreground">
                    "We’re putting the pieces together. Your video will be ready when it’s done."
                </p>
            </section>
            <section
                class="mt-10 rounded-[20px] border border-border bg-card p-5 sm:p-10"
                aria-label="Video production progress"
                data-project-id=(query.id.as_str())
                id="production-page"
            >
                <div class="flex items-center gap-4">
                    <div
                        class="flex size-[52px] shrink-0 items-center justify-center rounded-[14px] bg-accent-soft text-3xl text-accent-ink"
                        aria-hidden="true"
                    >
                        "✦"
                    </div>
                    <div class="min-w-0">
                        <h2 id="production-heading" class="text-xl font-bold">
                            "Creating your video"
                        </h2>
                        <p class="mt-1 break-all text-sm text-muted-foreground">
                            (project.title)
                        </p>
                    </div>
                </div>
                <div class="mt-8 flex flex-wrap items-center justify-between gap-2">
                    <h3 class="text-sm font-semibold">"Overall progress"</h3>
                    <span
                        id="production-state"
                        class="text-sm font-semibold text-accent-ink"
                    >
                        "In progress"
                    </span>
                </div>
                <div
                    id="production-bar"
                    class="production-progress mt-5"
                    role="progressbar"
                    aria-label="Video production in progress"
                >
                    <span></span>
                </div>
                <p
                    id="production-message"
                    role="status"
                    aria-live="polite"
                    class="mt-5 break-words text-sm text-muted-foreground"
                >
                    "Checking production status…"
                </p>
                <div class="mt-6 border-t border-border pt-6">
                    <h3 class="text-xs font-bold text-muted-foreground">"PRODUCTION STEPS"</h3>
                    <div
                        class="mt-5 flex flex-wrap items-center gap-x-4 gap-y-2 text-sm"
                    >
                        <span
                            class="production-step-icon production-step-complete"
                            aria-hidden="true"
                        >
                            "✓"
                        </span>
                        <span class="min-w-0 flex-1">"Preparing assets"</span>
                        <span class="text-xs text-success-ink">"Completed"</span>
                    </div>
                    <div
                        class="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 text-sm"
                    >
                        <span
                            id="production-step-icon"
                            class="production-step-icon production-step-active"
                            aria-hidden="true"
                        >
                            "◌"
                        </span>
                        <span class="min-w-0 flex-1 font-semibold">
                            "Producing video"
                        </span>
                        <span id="production-step-state" class="text-xs text-accent-ink">
                            "In progress"
                        </span>
                    </div>
                </div>
                <button
                    id="cancel-render"
                    type="button"
                    class="mt-8 min-h-[46px] w-full cursor-pointer rounded-[10px] border border-border bg-card px-4 text-sm font-semibold hover:bg-accent-soft focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8]"
                >
                    "Cancel production"
                </button>
                <button
                    id="retry-render"
                    type="button"
                    hidden="hidden"
                    class="mt-8 min-h-[46px] w-full cursor-pointer rounded-[10px] border border-accent-border bg-accent-soft px-4 text-sm font-semibold text-accent-ink hover:bg-accent-hover focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8]"
                >
                    "Retry production"
                </button>
                <a
                    href="/"
                    class="mt-8 flex min-h-[46px] w-full items-center justify-center rounded-[10px] border border-border bg-card px-4 text-sm font-semibold hover:bg-accent-soft focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8]"
                >
                    "Return to video setup"
                </a>
            </section>
            <p class="mt-6 text-center text-xs text-muted-foreground">
                "You can cancel production or return to video setup while it continues."
            </p>
        </main>
    })
}
