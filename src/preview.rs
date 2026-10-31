use crate::store::Store;
use futures_util::TryStreamExt;
use http_body::Frame;
use http_body_util::StreamBody;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{
        Body, error::bad_request, page, query_params, request::headers, response::Response, route,
    },
    view::{View, view},
};

#[query_params(error = bad_request)]
struct PreviewQuery {
    id: String,
    render: Option<u64>,
}

async fn select(cx: &Cx) -> Result<(String, crate::store::Project, u64)> {
    let query = query_params::<PreviewQuery>(cx)?;
    let project = app_context::<Arc<Store>>(cx)
        .load_project(crate::auth::username(cx), &query.id)
        .await
        .map_err(|_| bad_request("Project not found"))?;
    let render = match query.render {
        Some(number) => project
            .renders
            .iter()
            .find(|render| render.number == number),
        None => project.renders.last(),
    }
    .ok_or_else(|| bad_request("Render not found"))?
    .number;
    tracing::debug!(project_id = %query.id, render_number = render, "Render selected");
    Ok((query.id.clone(), project, render))
}

#[page]
async fn preview(cx: &Cx) -> Result<impl View> {
    let (id, project, number) = select(cx).await?;
    let url = format!("/preview/video?id={id}&render={number}");
    let back = format!("/?id={id}");
    let upload = format!("/youtube?id={id}&render={number}");
    Ok(view! {
        <main class="mx-auto w-full max-w-[1078px] px-6 pt-10 pb-16">
            <section class="sm:px-5">
                <p class="text-xs font-bold text-accent-ink">"YOUR FINISHED VIDEO"</p>
                <h1 class="mt-4 text-[30px] leading-[1.2] font-bold sm:text-[34px]">
                    "Ready for its close-up"
                </h1>
                <p class="mt-3 text-base text-muted-foreground">
                    "Preview your render, download it, or head back to make refinements."
                </p>
            </section>
            <section
                class="mt-9 rounded-[20px] border border-border bg-card p-5 sm:p-10"
            >
                <div class="flex flex-wrap items-center justify-between gap-4">
                    <h2 class="min-w-0 break-words text-xl font-bold">
                        (project.title)
                    </h2>
                    <span
                        class="rounded-full bg-accent-soft px-5 py-2 text-xs font-semibold text-accent-ink"
                    >
                        (format!("Render {}", number))
                    </span>
                </div>
                <video
                    class="mt-5 aspect-video w-full rounded-xl bg-[#111827]"
                    controls="controls"
                    preload="metadata"
                    src=(url.as_str())
                >
                    "Your browser does not support video playback."
                </video>
                <div class="mt-8 flex flex-col gap-4 sm:flex-row">
                    <a href=(upload) class="flex min-h-14 flex-1 items-center justify-center rounded-xl border border-border bg-card px-4 text-sm font-semibold cursor-pointer transition motion-reduce:transition-none hover:bg-accent-soft focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100">"Upload to YouTube"</a>
                    <a
                        href=(back)
                        class="flex min-h-14 flex-1 items-center justify-center rounded-xl border border-border bg-card px-4 text-sm font-semibold hover:bg-accent-soft focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8]"
                    >
                        "← Back to edit"
                    </a>
                    <a
                        href=(url.as_str())
                        download="download"
                        class="flex min-h-14 flex-1 items-center justify-center rounded-xl bg-[#6554e8] px-4 text-sm font-bold text-white hover:bg-[#5544d4] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8]"
                    >
                        "↓ Download video"
                    </a>
                </div>
            </section>
            <p class="mt-8 text-center text-sm text-muted-foreground">
                "Previous renders are saved. You can revisit or download them from the edit screen."
            </p>
        </main>
    })
}

#[route(GET "./video")]
async fn video(cx: &Cx) -> Result<Response> {
    let (id, project, number) = select(cx).await?;
    let render = project
        .renders
        .iter()
        .find(|render| render.number == number)
        .ok_or_else(|| bad_request("Render not found"))?;
    let store = app_context::<Arc<Store>>(cx);
    let root = tokio::fs::canonicalize(store.project_path(&id)).await?;
    let path = tokio::fs::canonicalize(root.join(&render.path)).await?;
    if !path.starts_with(&root) {
        return Err(bad_request("Invalid render path").into());
    }
    let content_type = match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        _ => "video/mp4",
    };
    let file = tokio::fs::File::open(path).await?;
    let range = if headers(cx).contains_key("if-range") {
        None
    } else {
        headers(cx)
            .get("range")
            .and_then(|value| value.to_str().ok())
    };
    video_response(file, content_type, range).await
}

#[derive(Debug, PartialEq)]
enum ByteRange {
    Full,
    Partial(u64, u64),
    Unsatisfiable,
}

fn parse_range(header: Option<&str>, length: u64) -> ByteRange {
    let Some((unit, value)) = header.and_then(|h| h.split_once('=')) else {
        return ByteRange::Full;
    };
    if !unit.eq_ignore_ascii_case("bytes") {
        return ByteRange::Full;
    }
    // Multiple ranges are deliberately ignored; a full response is valid.
    if value.contains(',') {
        return ByteRange::Full;
    }
    let Some((start, end)) = value.split_once('-') else {
        return ByteRange::Full;
    };
    if start.is_empty() {
        let Ok(suffix) = end.parse::<u64>() else {
            return ByteRange::Full;
        };
        if suffix == 0 || length == 0 {
            return ByteRange::Unsatisfiable;
        }
        return ByteRange::Partial(length.saturating_sub(suffix), length - 1);
    }
    let Ok(start) = start.parse::<u64>() else {
        return ByteRange::Full;
    };
    let end = if end.is_empty() {
        None
    } else {
        let Ok(value) = end.parse::<u64>() else {
            return ByteRange::Full;
        };
        Some(value)
    };
    if end.is_some_and(|end| end < start) {
        return ByteRange::Full;
    }
    if start >= length {
        return ByteRange::Unsatisfiable;
    }
    ByteRange::Partial(start, end.unwrap_or(length - 1).min(length - 1))
}

async fn video_response(
    mut file: tokio::fs::File,
    content_type: &str,
    range: Option<&str>,
) -> Result<Response> {
    let length = file.metadata().await?.len();
    let selection = parse_range(range, length);
    tracing::debug!(file_bytes = length, range_kind = ?selection, "Serving video");
    let mut response = Response::builder()
        .header("Accept-Ranges", "bytes")
        .header("Content-Type", content_type);
    let count = match selection {
        ByteRange::Full => length,
        ByteRange::Partial(start, end) => {
            file.seek(std::io::SeekFrom::Start(start)).await?;
            response = response
                .status(206)
                .header("Content-Range", format!("bytes {start}-{end}/{length}"));
            end - start + 1
        }
        ByteRange::Unsatisfiable => {
            return Ok(response
                .status(416)
                .header("Content-Range", format!("bytes */{length}"))
                .header("Content-Length", 0)
                .body(Body::empty())?);
        }
    };
    let stream = ReaderStream::new(file.take(count)).map_ok(Frame::data);
    Ok(response
        .header("Content-Length", count)
        .body(Body::new(StreamBody::new(stream)))?)
}

#[query_params(error = bad_request)]
struct ImageQuery {
    id: String,
    number: u64,
}

#[route(GET "./image")]
async fn image(cx: &Cx) -> Result<Response> {
    let query = query_params::<ImageQuery>(cx)?;
    let store = app_context::<Arc<Store>>(cx);
    let project = store
        .load_project(crate::auth::username(cx), &query.id)
        .await
        .map_err(|_| bad_request("Project not found"))?;
    let image = project
        .images
        .iter()
        .find(|image| image.number == query.number)
        .ok_or_else(|| bad_request("Image not found"))?;
    let root = tokio::fs::canonicalize(store.project_path(&query.id)).await?;
    let path = tokio::fs::canonicalize(root.join(&image.asset.path)).await?;
    if !path.starts_with(&root) {
        return Err(bad_request("Invalid image path").into());
    }
    let mime = match path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    let file = tokio::fs::File::open(path).await?;
    let length = file.metadata().await?.len();
    let stream = ReaderStream::new(file).map_ok(Frame::data);
    Ok(Response::builder()
        .header("Content-Type", mime)
        .header("Content-Length", length)
        .body(Body::new(StreamBody::new(stream)))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[test]
    fn range_selection_handles_invalid_and_unsatisfiable_requests() {
        assert_eq!(parse_range(None, 10), ByteRange::Full);
        assert_eq!(parse_range(Some("bytes=2-5"), 10), ByteRange::Partial(2, 5));
        assert_eq!(parse_range(Some("bytes=7-"), 10), ByteRange::Partial(7, 9));
        assert_eq!(parse_range(Some("bytes=-3"), 10), ByteRange::Partial(7, 9));
        assert_eq!(
            parse_range(Some("bytes=8-99"), 10),
            ByteRange::Partial(8, 9)
        );
        assert_eq!(parse_range(Some("bytes=-99"), 10), ByteRange::Partial(0, 9));
        assert_eq!(parse_range(Some("bytes=10-"), 10), ByteRange::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 10), ByteRange::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-"), 0), ByteRange::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=5-2"), 10), ByteRange::Full);
        assert_eq!(parse_range(Some("bytes=0-1,3-4"), 10), ByteRange::Full);
        assert_eq!(parse_range(Some("bytes=abc"), 10), ByteRange::Full);
    }

    #[tokio::test]
    async fn range_responses_stream_the_selected_bytes() {
        let path = std::env::temp_dir().join(format!("flicksy-range-test-{}", std::process::id()));
        tokio::fs::write(&path, b"0123456789").await.unwrap();
        for (range, status, content_range, expected) in [
            (None, 200, None, &b"0123456789"[..]),
            (Some("bytes=2-5"), 206, Some("bytes 2-5/10"), &b"2345"[..]),
            (Some("bytes=-3"), 206, Some("bytes 7-9/10"), &b"789"[..]),
            (Some("bytes=10-"), 416, Some("bytes */10"), &b""[..]),
        ] {
            let response = video_response(
                tokio::fs::File::open(&path).await.unwrap(),
                "video/mp4",
                range,
            )
            .await
            .unwrap();
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(response.headers().get("accept-ranges").unwrap(), "bytes");
            assert_eq!(
                response
                    .headers()
                    .get("content-range")
                    .map(|v| v.to_str().unwrap()),
                content_range
            );
            assert_eq!(
                response
                    .headers()
                    .get("content-length")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .parse::<usize>()
                    .unwrap(),
                expected.len()
            );
            assert_eq!(
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .as_ref(),
                expected
            );
        }
        tokio::fs::remove_file(path).await.unwrap();
    }
}
