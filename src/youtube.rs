use crate::{auth, config::YouTubeConfig, store::Store};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rig::rig_reqwest::reqwest::{self, StatusCode};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{
        Body, content::Json, error::bad_request, page, query_params, response::Response, route,
    },
    view::{View, view},
};

#[derive(Clone)]
pub struct YouTube {
    inner: Arc<Inner>,
}
struct Inner {
    config: Option<YouTubeConfig>,
    secret: String,
    cipher: Option<Aes256Gcm>,
    database: PathBuf,
    client: reqwest::Client,
    flows: Mutex<HashMap<String, Flow>>,
    active: Mutex<HashMap<String, String>>,
    connection_generation: Mutex<HashMap<String, u64>>,
    #[cfg(test)]
    test_origin: Mutex<Option<String>>,
}
struct Flow {
    user: String,
    session: String,
    verifier: String,
    project: String,
    render: u64,
    created: Instant,
    generation: u64,
}
fn ioerr(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn random() -> String {
    let mut b = [0u8; 32];
    b[..16].copy_from_slice(&argon2::password_hash::generate_salt());
    b[16..].copy_from_slice(&argon2::password_hash::generate_salt());
    URL_SAFE_NO_PAD.encode(b)
}
impl YouTube {
    pub fn new(config: Option<YouTubeConfig>, database: PathBuf) -> io::Result<Self> {
        let (secret, cipher) = if let Some(c) = &config {
            let uri = url::Url::parse(&c.redirect_uri).map_err(ioerr)?;
            if uri.scheme() != "https"
                && !(uri.scheme() == "http"
                    && matches!(uri.host_str(), Some("localhost" | "127.0.0.1" | "::1")))
            {
                return Err(ioerr(
                    "YouTube redirect_uri must use HTTPS (HTTP is allowed for localhost)",
                ));
            }
            if uri.path() != "/youtube/callback"
                || uri.query().is_some()
                || uri.fragment().is_some()
                || !uri.username().is_empty()
                || uri.password().is_some()
                || c.client_id.trim().is_empty()
            {
                return Err(ioerr("Invalid YouTube client ID or redirect URI"));
            }
            let secret = std::fs::read_to_string(&c.client_secret_file)?
                .trim()
                .to_owned();
            if secret.is_empty() {
                return Err(ioerr("Empty YouTube client secret"));
            }
            let key = URL_SAFE_NO_PAD
                .decode(std::fs::read_to_string(&c.encryption_key_file)?.trim())
                .map_err(|_| ioerr("YouTube encryption key must be unpadded base64url"))?;
            (
                secret,
                Some(
                    Aes256Gcm::new_from_slice(&key)
                        .map_err(|_| ioerr("YouTube encryption key must contain 32 bytes"))?,
                ),
            )
        } else {
            (String::new(), None)
        };
        let this = Self {
            inner: Arc::new(Inner {
                config,
                secret,
                cipher,
                database,
                client: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(15))
                    .timeout(Duration::from_secs(180))
                    .build()
                    .map_err(ioerr)?,
                flows: Mutex::new(HashMap::new()),
                active: Mutex::new(HashMap::new()),
                connection_generation: Mutex::new(HashMap::new()),
                #[cfg(test)]
                test_origin: Mutex::new(None),
            }),
        };
        this.db()?.execute_batch("CREATE TABLE IF NOT EXISTS youtube_connections (username TEXT PRIMARY KEY REFERENCES users(username) ON DELETE CASCADE, token BLOB NOT NULL, channel_id TEXT NOT NULL, channel_title TEXT NOT NULL); CREATE TABLE IF NOT EXISTS youtube_uploads (id TEXT PRIMARY KEY, username TEXT NOT NULL REFERENCES users(username) ON DELETE CASCADE, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT, render INTEGER NOT NULL, title TEXT NOT NULL, description TEXT NOT NULL, privacy TEXT NOT NULL, made_for_kids INTEGER NOT NULL, state TEXT NOT NULL, session BLOB, sent INTEGER NOT NULL DEFAULT 0, total INTEGER NOT NULL DEFAULT 0, video_id TEXT, error TEXT, actual_privacy TEXT, channel_id TEXT NOT NULL); CREATE UNIQUE INDEX IF NOT EXISTS youtube_pending_render ON youtube_uploads(username,project_id,render) WHERE state IN ('queued','uploading','interrupted');").map_err(ioerr)?;
        Ok(this)
    }
    fn db(&self) -> io::Result<Connection> {
        let c = Connection::open(&self.inner.database).map_err(ioerr)?;
        c.busy_timeout(Duration::from_secs(5)).map_err(ioerr)?;
        c.pragma_update(None, "foreign_keys", "ON").map_err(ioerr)?;
        Ok(c)
    }
    fn endpoint(&self, path: &str, default: &str) -> String {
        #[cfg(test)]
        if let Some(origin) = self.inner.test_origin.lock().unwrap().as_ref() {
            return format!("{origin}{path}");
        }
        let _ = path;
        default.to_owned()
    }
    fn session_allowed(&self, value: &str) -> bool {
        #[cfg(test)]
        if let Some(origin) = self.inner.test_origin.lock().unwrap().as_ref() {
            return value == format!("{origin}/session");
        }
        valid_session_url(value)
    }
    fn seal(&self, user: &str, value: &str) -> io::Result<Vec<u8>> {
        let c = self
            .inner
            .cipher
            .as_ref()
            .ok_or_else(|| ioerr("YouTube not configured"))?;
        let r = argon2::password_hash::generate_salt();
        let mut out = r[..12].to_vec();
        let bytes = c
            .encrypt(
                Nonce::from_slice(&out),
                Payload {
                    msg: value.as_bytes(),
                    aad: user.as_bytes(),
                },
            )
            .map_err(|_| ioerr("Encryption failed"))?;
        out.extend(bytes);
        Ok(out)
    }
    fn open(&self, user: &str, value: &[u8]) -> io::Result<String> {
        if value.len() < 12 {
            return Err(ioerr("Invalid encrypted credential"));
        }
        let c = self
            .inner
            .cipher
            .as_ref()
            .ok_or_else(|| ioerr("YouTube not configured"))?;
        String::from_utf8(
            c.decrypt(
                Nonce::from_slice(&value[..12]),
                Payload {
                    msg: &value[12..],
                    aad: user.as_bytes(),
                },
            )
            .map_err(|_| ioerr("Unable to decrypt credential"))?,
        )
        .map_err(ioerr)
    }
    fn config(&self) -> io::Result<&YouTubeConfig> {
        self.inner
            .config
            .as_ref()
            .ok_or_else(|| ioerr("YouTube is not configured"))
    }
    fn connection(&self, user: &str) -> io::Result<Option<(Vec<u8>, String, String)>> {
        self.db()?
            .query_row(
                "SELECT token,channel_id,channel_title FROM youtube_connections WHERE username=?1",
                [user],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(ioerr)
    }
    fn save_connection(
        &self,
        user: &str,
        session: &str,
        token: &[u8],
        channel: &str,
        title: &str,
    ) -> io::Result<bool> {
        let mut db = self.db()?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(ioerr)?;
        let session_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE token_hash=?1 AND username=?2)",
                params![session, user],
                |r| r.get(0),
            )
            .map_err(ioerr)?;
        if !session_exists {
            return Ok(false);
        }
        tx.execute("INSERT INTO youtube_connections(username,token,channel_id,channel_title) VALUES (?1,?2,?3,?4) ON CONFLICT(username) DO UPDATE SET token=excluded.token,channel_id=excluded.channel_id,channel_title=excluded.channel_title",params![user,token,channel,title]).map_err(ioerr)?;
        tx.commit().map_err(ioerr)?;
        Ok(true)
    }
    async fn access_token(&self, user: &str) -> io::Result<String> {
        let c = self.config()?;
        let (token, _, _) = self
            .connection(user)?
            .ok_or_else(|| ioerr("Connect YouTube first"))?;
        let refresh = self.open(user, &token)?;
        let r = self
            .inner
            .client
            .post(self.endpoint("/token", "https://oauth2.googleapis.com/token"))
            .form(&[
                ("client_id", c.client_id.as_str()),
                ("client_secret", self.inner.secret.as_str()),
                ("refresh_token", refresh.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .map_err(|_| ioerr("Google token service unavailable"))?;
        if !r.status().is_success() {
            return Err(ioerr(
                "YouTube authorization expired or was revoked; reconnect",
            ));
        }
        let v: serde_json::Value = r
            .json()
            .await
            .map_err(|_| ioerr("Invalid Google token response"))?;
        v["access_token"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| ioerr("Missing Google access token"))
    }
}
fn redirect(location: &str) -> Result<Response> {
    Ok(Response::builder()
        .status(303)
        .header("Location", location)
        .header("Referrer-Policy", "no-referrer")
        .header("Cache-Control", "no-store")
        .body(Body::empty())?)
}
#[query_params(error = bad_request)]
struct Selection {
    id: String,
    render: u64,
    error: Option<String>,
}
async fn render_file(
    store: &Store,
    user: &str,
    id: &str,
    number: u64,
) -> io::Result<(String, PathBuf)> {
    let project = store.load_project(user, id).await?;
    let render = project
        .renders
        .iter()
        .find(|r| r.number == number)
        .ok_or_else(|| ioerr("Render not found"))?;
    let root = tokio::fs::canonicalize(store.project_path(id)).await?;
    let path = tokio::fs::canonicalize(root.join(&render.path)).await?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(ioerr("Invalid render file"));
    }
    Ok((project.title, path))
}

#[page("/youtube")]
async fn youtube_page(cx: &Cx) -> Result<impl View> {
    let q = query_params::<Selection>(cx)?;
    let user = auth::username(cx);
    let yt = app_context::<YouTube>(cx);
    let store = app_context::<Arc<Store>>(cx);
    let (title, _) = render_file(store, user, &q.id, q.render)
        .await
        .map_err(|_| bad_request("Render not found"))?;
    let enabled = yt.inner.config.is_some();
    let connection = yt.connection(user)?;
    let connected = connection.is_some();
    let channel = connection.map(|c| c.2).unwrap_or_default();
    let back = format!("/preview?id={}&render={}", q.id, q.render);
    Ok(
        view! { <main class="mx-auto max-w-[824px] px-6 pt-10 pb-16"><h1 class="text-3xl font-bold">"Upload to YouTube"</h1><p class="mt-3 text-muted-foreground">"Review the details for this render before uploading."</p>
        <section id="youtube-page" data-project=(q.id.clone()) data-render=(q.render.to_string()) class="mt-8 rounded-[20px] bg-card p-6 sm:p-10">
        if q.error.as_deref() == Some("denied") { <p role="alert" class="mb-4 text-danger-ink">"Google authorization was cancelled or denied. You can try connecting again."</p> }
        if !enabled { <p>"YouTube integration is not configured on this server."</p> }
        else { if connected { <p>"Connected channel: " <strong>(channel)</strong></p> <form method="post" action="/youtube/disconnect" class="mt-3"><button class="text-accent-ink underline" type="submit">"Disconnect YouTube"</button></form> }
        else { <form method="post" action="/youtube/connect"><input type="hidden" name="id" value=(q.id.clone())><input type="hidden" name="render" value=(q.render.to_string())><button class="rounded-xl bg-[#6554e8] px-4 py-3 font-bold text-white cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#5544d4] active:bg-[#4939bd]" type="submit">"Connect YouTube"</button></form> }
        if connected { <form id="youtube-upload-form" class="mt-6 flex flex-col gap-4" data-project=(q.id.clone()) data-render=(q.render.to_string())>
        <label class="flex flex-col gap-2">"Title (up to 100 characters)" <input name="title" required="required" value=(title) class="rounded-xl border border-border bg-surface p-3"></label>
        <label class="flex flex-col gap-2">"Description" <textarea name="description" rows="5" class="rounded-xl border border-border bg-surface p-3"></textarea></label>
        <label class="flex flex-col gap-2">"Visibility" <select name="privacy" class="rounded-xl border border-border bg-surface p-3"><option value="private">"Private"</option><option value="unlisted">"Unlisted"</option><option value="public">"Public"</option></select></label>
        <label class="flex flex-col gap-2">"Is this video made for kids?" <select name="made_for_kids" required="required" class="rounded-xl border border-border bg-surface p-3"><option value="">"Choose an audience"</option><option value="true">"Yes, made for kids"</option><option value="false">"No, not made for kids"</option></select></label>
        <label><input type="checkbox" name="certify" required="required">" I confirm this video complies with " <a href="https://www.youtube.com/howyoutubeworks/policies/community-guidelines/" target="_blank" rel="noopener noreferrer" class="text-accent-ink underline">"YouTube's Community Guidelines"</a>"."</label>
        <button type="submit" class="rounded-xl bg-[#6554e8] px-4 py-3 font-bold text-white cursor-pointer transition motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100 hover:bg-[#5544d4] active:bg-[#4939bd] disabled:cursor-not-allowed disabled:opacity-60">"Upload to YouTube"</button></form> }
        }
        <p id="youtube-status" role="status" aria-live="polite" class="mt-4"></p> <a id="youtube-link" hidden="hidden" target="_blank" rel="noopener noreferrer" class="mt-4 inline-block text-accent-ink underline">"Open on YouTube"</a>
        <div id="youtube-history" class="mt-4"></div>
        <a href=(back) class="mt-6 inline-block text-accent-ink underline">"Back to preview"</a></section></main> },
    )
}
#[page("/youtube/settings")]
async fn settings(cx: &Cx) -> Result<impl View> {
    let yt = app_context::<YouTube>(cx);
    let connection = yt.connection(auth::username(cx))?;
    let enabled = yt.inner.config.is_some();
    Ok(
        view! { <main class="mx-auto max-w-[824px] px-6 pt-10 pb-16"><h1 class="text-3xl font-bold">"YouTube connection"</h1><section class="mt-8 rounded-[20px] bg-card p-6 sm:p-10">
        if !enabled { <p>"YouTube integration is not configured on this server."</p> }
        else if let Some((_, _, title)) = connection { <p>"Connected channel: " <strong>(title)</strong></p><form method="post" action="/youtube/disconnect" class="mt-4"><button type="submit" class="rounded-xl border border-border px-4 py-2 font-semibold">"Disconnect YouTube"</button></form><p class="mt-4 text-sm text-muted-foreground">"Disconnecting removes Flicksy's stored authorization. You can also revoke access in " <a class="text-accent-ink underline" href="https://myaccount.google.com/permissions" target="_blank" rel="noopener noreferrer">"your Google account"</a>"."</p> }
        else { <p>"No YouTube channel connected. Open a video preview to connect and upload."</p> }
        <a class="mt-6 inline-block text-accent-ink underline" href="/">"Back to projects"</a></section></main> },
    )
}
#[derive(Deserialize)]
struct Connect {
    id: String,
    render: u64,
}
#[route(POST "/youtube/connect")]
async fn connect(
    cx: &Cx,
    topcoat::router::content::Form(q): topcoat::router::content::Form<Connect>,
) -> Result<Response> {
    let yt = app_context::<YouTube>(cx);
    let user = auth::username(cx);
    let store = app_context::<Arc<Store>>(cx);
    render_file(store, user, &q.id, q.render)
        .await
        .map_err(|_| bad_request("Render not found"))?;
    let c = yt
        .config()
        .map_err(|_| bad_request("YouTube is not configured"))?;
    let session = auth::session_fingerprint(cx).ok_or_else(|| bad_request("Session not found"))?;
    let state = random();
    let verifier = random();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let generation = {
        let mut generations = yt.inner.connection_generation.lock().unwrap();
        let generation = generations.entry(user.to_owned()).or_default();
        *generation = generation.wrapping_add(1);
        *generation
    };
    let mut flows = yt.inner.flows.lock().unwrap();
    flows.retain(|_, f| f.created.elapsed() < Duration::from_secs(600));
    flows.insert(
        state.clone(),
        Flow {
            user: user.to_owned(),
            session,
            verifier,
            project: q.id,
            render: q.render,
            created: Instant::now(),
            generation,
        },
    );
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")?;
    url.query_pairs_mut().extend_pairs([("client_id",c.client_id.as_str()),("redirect_uri",c.redirect_uri.as_str()),("response_type","code"),("scope","https://www.googleapis.com/auth/youtube.upload https://www.googleapis.com/auth/youtube.readonly"),("access_type","offline"),("prompt","consent"),("state",state.as_str()),("code_challenge",challenge.as_str()),("code_challenge_method","S256")]);
    redirect(url.as_str())
}
#[query_params(error = bad_request)]
struct Callback {
    state: String,
    code: Option<String>,
    error: Option<String>,
}
#[route(GET "/youtube/callback")]
async fn callback(cx: &Cx) -> Result<Response> {
    let q = query_params::<Callback>(cx)?;
    let yt = app_context::<YouTube>(cx);
    let f = yt
        .inner
        .flows
        .lock()
        .unwrap()
        .remove(&q.state)
        .ok_or_else(|| bad_request("OAuth request expired"))?;
    if f.created.elapsed() > Duration::from_secs(600)
        || f.user != auth::username(cx)
        || Some(f.session.as_str()) != auth::session_fingerprint(cx).as_deref()
    {
        return Err(bad_request("OAuth session mismatch").into());
    }
    let back = format!("/youtube?id={}&render={}", f.project, f.render);
    if q.error.is_some() {
        return redirect(&format!("{back}&error=denied"));
    }
    let code = q
        .code
        .as_ref()
        .ok_or_else(|| bad_request("Authorization code missing"))?;
    let c = yt.config()?;
    let resp = yt
        .inner
        .client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code.as_str()),
            ("client_id", c.client_id.as_str()),
            ("client_secret", yt.inner.secret.as_str()),
            ("redirect_uri", c.redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", f.verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|_| bad_request("Google authorization failed"))?;
    if !resp.status().is_success() {
        return Err(bad_request("Google authorization failed").into());
    }
    let token: serde_json::Value = resp
        .json()
        .await
        .map_err(|_| bad_request("Invalid Google response"))?;
    let access = token["access_token"]
        .as_str()
        .ok_or_else(|| bad_request("Missing access token"))?;
    let refresh = token["refresh_token"]
        .as_str()
        .ok_or_else(|| bad_request("Google did not provide a refresh token; reconnect"))?;
    let response = yt
        .inner
        .client
        .get("https://www.googleapis.com/youtube/v3/channels")
        .query(&[("part", "snippet"), ("mine", "true")])
        .bearer_auth(access)
        .send()
        .await
        .map_err(|_| bad_request("Unable to identify the YouTube channel"))?;
    if !response.status().is_success() {
        return Err(bad_request("Unable to identify the YouTube channel").into());
    }
    let data: serde_json::Value = response
        .json()
        .await
        .map_err(|_| bad_request("Invalid channel response"))?;
    let items = data["items"]
        .as_array()
        .ok_or_else(|| bad_request("No YouTube channel found"))?;
    if items.len() != 1 {
        return Err(bad_request("Select a single YouTube channel when connecting").into());
    }
    let id = items[0]["id"]
        .as_str()
        .ok_or_else(|| bad_request("Channel ID missing"))?;
    let title = items[0]["snippet"]["title"]
        .as_str()
        .ok_or_else(|| bad_request("Channel title missing"))?;
    let encrypted = yt.seal(&f.user, refresh)?;
    let active = yt.inner.active.lock().unwrap();
    if active.contains_key(&f.user) {
        return Err(
            bad_request("Wait for the upload to finish before changing the connection").into(),
        );
    }
    let generations = yt.inner.connection_generation.lock().unwrap();
    if generations.get(&f.user).copied() != Some(f.generation) {
        return Err(bad_request("OAuth request was superseded; connect again").into());
    }
    if !yt.save_connection(&f.user, &f.session, &encrypted, id, title)? {
        return Err(bad_request("OAuth session expired; connect again").into());
    }
    redirect(&back)
}
#[route(POST "/youtube/disconnect")]
async fn disconnect(cx: &Cx) -> Result<Response> {
    let yt = app_context::<YouTube>(cx);
    let user = auth::username(cx);
    let active = yt.inner.active.lock().unwrap();
    if active.contains_key(user) {
        return Err(bad_request("Wait for the upload to finish before disconnecting").into());
    }
    let mut generations = yt.inner.connection_generation.lock().unwrap();
    let generation = generations.entry(user.to_owned()).or_default();
    *generation = generation.wrapping_add(1);
    yt.db()?
        .execute("DELETE FROM youtube_connections WHERE username=?1", [user])
        .map_err(ioerr)?;
    redirect("/")
}

#[derive(Deserialize)]
struct UploadRequest {
    id: String,
    render: u64,
    title: String,
    description: String,
    privacy: String,
    made_for_kids: bool,
    certify: bool,
}
#[derive(Serialize)]
struct UploadStatus {
    id: String,
    state: String,
    sent: i64,
    total: i64,
    video_id: Option<String>,
    error: Option<String>,
    actual_privacy: Option<String>,
}
fn upload_status(yt: &YouTube, user: &str, id: &str) -> io::Result<UploadStatus> {
    yt.db()?.query_row("SELECT id,state,sent,total,video_id,error,actual_privacy FROM youtube_uploads WHERE id=?1 AND username=?2",params![id,user],|r|Ok(UploadStatus{id:r.get(0)?,state:r.get(1)?,sent:r.get(2)?,total:r.get(3)?,video_id:r.get(4)?,error:r.get(5)?,actual_privacy:r.get(6)?})).map_err(ioerr)
}
fn launch(yt: YouTube, store: Arc<Store>, user: String, id: String) -> io::Result<()> {
    {
        let mut active = yt.inner.active.lock().unwrap();
        if active.contains_key(&user) {
            return Err(ioerr("An upload is already in progress"));
        }
        active.insert(user.clone(), id.clone());
    }
    if let Err(e) = yt.db().and_then(|db| {
        db.execute(
            "UPDATE youtube_uploads SET state='queued',error=NULL WHERE id=?1 AND username=?2",
            params![id, user],
        )
        .map_err(ioerr)
    }) {
        yt.inner.active.lock().unwrap().remove(&user);
        return Err(e);
    }
    tokio::spawn(async move {
        if let Err(e) = run_upload(&yt, &store, &user, &id).await {
            tracing::warn!("YouTube upload stopped: {}", e);
            if let Ok(db) = yt.db() {
                let _=db.execute("UPDATE youtube_uploads SET state='interrupted',error=?1 WHERE id=?2 AND username=?3 AND state!='complete'",params![e.to_string(),id,user]);
            }
        }
        yt.inner.active.lock().unwrap().remove(&user);
    });
    Ok(())
}
#[route(POST "/youtube/upload")]
async fn start_upload(cx: &Cx, Json(q): Json<UploadRequest>) -> Result<Json<UploadStatus>> {
    let user = auth::username(cx);
    let yt = app_context::<YouTube>(cx);
    let store = app_context::<Arc<Store>>(cx);
    yt.config()
        .map_err(|_| bad_request("YouTube not configured"))?;
    if !q.certify
        || q.title.trim().is_empty()
        || q.title.chars().count() > 100
        || q.description.len() > 5000
        || q.title.contains(['<', '>'])
        || q.description.contains(['<', '>'])
        || !matches!(q.privacy.as_str(), "private" | "unlisted" | "public")
    {
        return Err(bad_request("Invalid upload details").into());
    }
    let (_, path) = render_file(store, user, &q.id, q.render)
        .await
        .map_err(|_| bad_request("Render not found"))?;
    let (_, channel, _) = yt
        .connection(user)?
        .ok_or_else(|| bad_request("Connect YouTube first"))?;
    let total = tokio::fs::metadata(path).await?.len();
    if total == 0 || total > 256_000_000_000 {
        return Err(bad_request("Invalid video size").into());
    }
    if yt.inner.active.lock().unwrap().contains_key(user) {
        return Err(bad_request("An upload is already in progress").into());
    }
    let pending: bool = yt.db()?.query_row("SELECT EXISTS(SELECT 1 FROM youtube_uploads WHERE username=?1 AND project_id=?2 AND render=?3 AND state IN ('queued','uploading','interrupted'))", params![user,q.id,q.render as i64], |r|r.get(0)).map_err(ioerr)?;
    if pending {
        return Err(bad_request("Resume or abandon the previous upload first").into());
    }
    let id = random();
    yt.db()?.execute("INSERT INTO youtube_uploads(id,username,project_id,render,title,description,privacy,made_for_kids,state,total,channel_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'queued',?9,?10)",params![id,user,q.id,q.render as i64,q.title,q.description,q.privacy,q.made_for_kids,total as i64,channel]).map_err(ioerr)?;
    if launch(yt.clone(), store.clone(), user.to_owned(), id.clone()).is_err() {
        yt.db()?
            .execute(
                "DELETE FROM youtube_uploads WHERE id=?1 AND username=?2",
                params![id, user],
            )
            .map_err(ioerr)?;
        return Err(bad_request("An upload is already in progress").into());
    }
    Ok(Json(upload_status(yt, user, &id)?))
}
#[query_params(error = bad_request)]
struct StatusQuery {
    id: String,
}
#[route(GET "/youtube/status")]
async fn youtube_status(cx: &Cx) -> Result<Json<UploadStatus>> {
    let q = query_params::<StatusQuery>(cx)?;
    let yt = app_context::<YouTube>(cx);
    let mut status = upload_status(yt, auth::username(cx), &q.id)
        .map_err(|_| bad_request("Upload not found"))?;
    if (status.state == "uploading" || status.state == "queued")
        && yt.inner.active.lock().unwrap().get(auth::username(cx)) != Some(&q.id)
    {
        status.state = "interrupted".into();
    }
    Ok(Json(status))
}
#[route(POST "/youtube/resume")]
async fn resume(cx: &Cx, Json(q): Json<StatusQuery>) -> Result<Json<UploadStatus>> {
    let yt = app_context::<YouTube>(cx);
    let user = auth::username(cx);
    let status = upload_status(yt, user, &q.id).map_err(|_| bad_request("Upload not found"))?;
    if status.state == "complete" {
        return Ok(Json(status));
    }
    if status.state == "abandoned" {
        return Err(bad_request("Upload was abandoned").into());
    }
    let store = app_context::<Arc<Store>>(cx);
    launch(yt.clone(), store.clone(), user.to_owned(), q.id.clone())
        .map_err(|_| bad_request("An upload is already in progress"))?;
    Ok(Json(upload_status(yt, user, &q.id)?))
}
#[route(POST "/youtube/abandon")]
async fn abandon(cx: &Cx, Json(q): Json<StatusQuery>) -> Result<Json<UploadStatus>> {
    let yt = app_context::<YouTube>(cx);
    let user = auth::username(cx);
    let active = yt.inner.active.lock().unwrap();
    if active.get(user) == Some(&q.id) {
        return Err(bad_request("Upload is still in progress").into());
    }
    let changed = yt.db()?.execute("UPDATE youtube_uploads SET state='abandoned',session=NULL WHERE id=?1 AND username=?2 AND state IN ('queued','uploading','interrupted')", params![q.id,user]).map_err(ioerr)?;
    if changed != 1 {
        return Err(bad_request("Upload cannot be abandoned").into());
    }
    Ok(Json(upload_status(yt, user, &q.id)?))
}
fn valid_session_url(value: &str) -> bool {
    url::Url::parse(value).ok().is_some_and(|u| {
        u.scheme() == "https"
            && u.host_str()
                .is_some_and(|h| h == "www.googleapis.com" || h.ends_with(".googleapis.com"))
            && u.username().is_empty()
            && u.password().is_none()
    })
}
async fn run_upload(yt: &YouTube, store: &Store, user: &str, id: &str) -> io::Result<()> {
    let row:(String,i64,String,String,String,bool,Option<Vec<u8>>,i64,String)=yt.db()?.query_row("SELECT project_id,render,title,description,privacy,made_for_kids,session,total,channel_id FROM youtube_uploads WHERE id=?1 AND username=?2",params![id,user],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(ioerr)?;
    let (_, path) = render_file(store, user, &row.0, row.1 as u64).await?;
    let content_type = match path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        _ => "video/mp4",
    };
    let mut file = tokio::fs::File::open(path).await?;
    let total = file.metadata().await?.len();
    if total != row.7 as u64 {
        return Err(ioerr("Render file changed; upload cannot be resumed"));
    }
    let (_, current_channel, _) = yt
        .connection(user)?
        .ok_or_else(|| ioerr("Connect YouTube first"))?;
    if current_channel != row.8 {
        return Err(ioerr(
            "The connected channel changed; reconnect the original channel to resume",
        ));
    }
    let access = yt.access_token(user).await?;
    yt.db()?
        .execute(
            "UPDATE youtube_uploads SET state='uploading',error=NULL WHERE id=?1 AND username=?2",
            params![id, user],
        )
        .map_err(ioerr)?;
    let session = if let Some(saved) = row.6 {
        yt.open(user, &saved)?
    } else {
        let metadata = serde_json::json!({"snippet":{"title":row.2,"description":row.3},"status":{"privacyStatus":row.4,"selfDeclaredMadeForKids":row.5}});
        let response=yt.inner.client.post(yt.endpoint("/videos", "https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet%2Cstatus")).bearer_auth(&access).header("X-Upload-Content-Length",total).header("X-Upload-Content-Type",content_type).json(&metadata).send().await.map_err(|_|ioerr("Could not start YouTube upload"))?;
        if !response.status().is_success() {
            return Err(ioerr(format!(
                "YouTube rejected upload setup ({})",
                response.status().as_u16()
            )));
        }
        let url = response
            .headers()
            .get("location")
            .and_then(|s| s.to_str().ok())
            .ok_or_else(|| ioerr("YouTube did not return an upload session"))?
            .to_owned();
        if !yt.session_allowed(&url) {
            return Err(ioerr("Invalid YouTube upload session URL"));
        }
        let encrypted = yt.seal(user, &url)?;
        yt.db()?
            .execute(
                "UPDATE youtube_uploads SET session=?1 WHERE id=?2 AND username=?3",
                params![encrypted, id, user],
            )
            .map_err(ioerr)?;
        url
    };
    if !yt.session_allowed(&session) {
        return Err(ioerr("Invalid YouTube upload session URL"));
    }
    let mut previous_offset = None;
    let mut stagnant = 0;
    let mut failures = 0;
    loop {
        let probe = yt
            .inner
            .client
            .put(&session)
            .bearer_auth(&access)
            .header("Content-Length", "0")
            .header("Content-Range", format!("bytes */{total}"))
            .send()
            .await
            .map_err(|_| ioerr("Could not check upload progress; resume to retry"))?;
        if probe.status().is_success() {
            return complete(yt, user, id, probe).await;
        }
        if probe.status() == StatusCode::NOT_FOUND {
            return Err(ioerr(
                "YouTube upload session expired; check YouTube Studio before starting a new upload",
            ));
        }
        if probe.status().as_u16() != 308 {
            return Err(ioerr(format!(
                "YouTube progress check failed ({})",
                probe.status().as_u16()
            )));
        }
        let offset = if let Some(range) = probe.headers().get("range").and_then(|v| v.to_str().ok())
        {
            range
                .strip_prefix("bytes=0-")
                .and_then(|s| s.parse::<u64>().ok())
                .and_then(|v| v.checked_add(1))
                .ok_or_else(|| ioerr("Invalid YouTube upload range"))?
        } else {
            0
        };
        if offset >= total {
            return Err(ioerr(
                "Upload completion needs confirmation; resume to check",
            ));
        }
        yt.db()?
            .execute(
                "UPDATE youtube_uploads SET sent=?1 WHERE id=?2 AND username=?3",
                params![offset as i64, id, user],
            )
            .map_err(ioerr)?;
        if previous_offset == Some(offset) {
            stagnant += 1;
        } else {
            stagnant = 0;
        }
        if stagnant >= 3 {
            return Err(ioerr(
                "YouTube did not acknowledge upload progress; resume to retry",
            ));
        }
        previous_offset = Some(offset);
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        let size = (total - offset).min(8 * 1024 * 1024) as usize;
        let mut bytes = vec![0; size];
        file.read_exact(&mut bytes).await?;
        let result = yt
            .inner
            .client
            .put(&session)
            .bearer_auth(&access)
            .header("Content-Type", content_type)
            .header(
                "Content-Range",
                format!("bytes {}-{}/{}", offset, offset + size as u64 - 1, total),
            )
            .body(bytes)
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => {
                return complete(yt, user, id, response).await;
            }
            Ok(response) if response.status().as_u16() == 308 => {
                failures = 0;
            }
            Ok(response) if response.status().is_server_error() => {
                failures += 1;
            }
            Err(_) => {
                failures += 1;
            }
            Ok(response) => {
                return Err(ioerr(format!(
                    "YouTube upload failed ({})",
                    response.status().as_u16()
                )));
            }
        }
        if failures >= 3 {
            return Err(ioerr("Upload interrupted; resume to retry"));
        }
        if failures > 0 {
            tokio::time::sleep(Duration::from_secs(2u64.pow(failures))).await;
        }
    }
}
async fn complete(
    yt: &YouTube,
    user: &str,
    id: &str,
    response: reqwest::Response,
) -> io::Result<()> {
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|_| ioerr("Could not confirm uploaded video; resume to check"))?;
    let video = value["id"]
        .as_str()
        .filter(|v| {
            !v.is_empty()
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .ok_or_else(|| ioerr("YouTube did not return a video ID; resume to check"))?;
    let visibility = value["status"]["privacyStatus"]
        .as_str()
        .filter(|v| matches!(*v, "private" | "public" | "unlisted"));
    yt.db()?.execute("UPDATE youtube_uploads SET state='complete',video_id=?1,actual_privacy=?2,sent=total,session=NULL,error=NULL WHERE id=?3 AND username=?4",params![video,visibility,id,user]).map_err(ioerr)?;
    Ok(())
}
#[route(GET "/youtube/history")]
async fn history(cx: &Cx) -> Result<Json<Vec<UploadStatus>>> {
    let q = query_params::<Selection>(cx)?;
    let user = auth::username(cx);
    let store = app_context::<Arc<Store>>(cx);
    render_file(store, user, &q.id, q.render)
        .await
        .map_err(|_| bad_request("Render not found"))?;
    let yt = app_context::<YouTube>(cx);
    let db = yt.db()?;
    let mut stmt=db.prepare("SELECT id,state,sent,total,video_id,error,actual_privacy FROM youtube_uploads WHERE username=?1 AND project_id=?2 AND render=?3 ORDER BY rowid DESC").map_err(ioerr)?;
    let active = yt.inner.active.lock().unwrap().get(user).cloned();
    let results = stmt
        .query_map(params![user, q.id, q.render as i64], |r| {
            let mut state: String = r.get(1)?;
            if active.as_deref() != Some(r.get::<_, String>(0)?.as_str())
                && (state == "queued" || state == "uploading")
            {
                state = "interrupted".into();
            }
            Ok(UploadStatus {
                id: r.get(0)?,
                state,
                sent: r.get(2)?,
                total: r.get(3)?,
                video_id: r.get(4)?,
                error: r.get(5)?,
                actual_privacy: r.get(6)?,
            })
        })
        .map_err(ioerr)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(ioerr)?;
    Ok(Json(results))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires permission to bind a loopback test server"]
    async fn resumes_after_server_error_without_reuploading_acknowledged_bytes() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let fake_origin = origin.clone();
        let server = std::thread::spawn(move || {
            let mut ranges = Vec::new();
            for step in 0..6 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(15)))
                    .unwrap();
                let mut data = Vec::new();
                let header_end = loop {
                    let mut buf = [0; 8192];
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&buf[..n]);
                    if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                };
                let header = String::from_utf8_lossy(&data[..header_end]).to_lowercase();
                let len: usize = header
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .and_then(|v| v.trim().parse().ok())
                    })
                    .unwrap_or(0);
                while data.len() - header_end < len {
                    let mut buf = [0; 65536];
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&buf[..n]);
                }
                if let Some(range) = header
                    .lines()
                    .find(|line| line.starts_with("content-range:"))
                {
                    ranges.push(range.to_owned());
                }
                let (status, extra, body) = match step {
                    0 => (
                        "200 OK",
                        String::new(),
                        "{\"access_token\":\"test-access\"}",
                    ),
                    1 => ("200 OK", format!("Location: {fake_origin}/session\r\n"), ""),
                    2 => ("308 Resume Incomplete", String::new(), ""),
                    3 => ("500 Internal Server Error", String::new(), ""),
                    4 => (
                        "308 Resume Incomplete",
                        "Range: bytes=0-8388607\r\n".to_string(),
                        "",
                    ),
                    5 => (
                        "201 Created",
                        String::new(),
                        "{\"id\":\"video123\",\"status\":{\"privacyStatus\":\"private\"}}",
                    ),
                    _ => unreachable!(),
                };
                write!(stream,"HTTP/1.1 {status}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
            ranges
        });
        let root = std::env::temp_dir().join(format!("flicksy-youtube-resume-{}", random()));
        std::fs::create_dir_all(&root).unwrap();
        let database = root.join("db.sqlite");
        let accounts = crate::accounts::Accounts::open(&database, false).unwrap();
        accounts.add("alice", "password").unwrap();
        accounts.add("bob", "password").unwrap();
        let store = Store::open(root.join("projects"), database.clone()).unwrap();
        let pending = store.begin().unwrap();
        std::fs::write(
            pending.path.join("video.mp4"),
            vec![7u8; 8 * 1024 * 1024 + 3],
        )
        .unwrap();
        let project = crate::store::Project {
            title: "Test".into(),
            ..Default::default()
        };
        let project_id = store.commit(pending, "alice", &project).await.unwrap();
        store
            .record_render("alice", &project_id, "video.mp4")
            .await
            .unwrap();
        std::fs::write(root.join("secret"), "secret").unwrap();
        std::fs::write(root.join("key"), URL_SAFE_NO_PAD.encode([3u8; 32])).unwrap();
        let yt = YouTube::new(
            Some(YouTubeConfig {
                client_id: "id".into(),
                client_secret_file: root.join("secret"),
                encryption_key_file: root.join("key"),
                redirect_uri: "https://example.com/youtube/callback".into(),
            }),
            database,
        )
        .unwrap();
        *yt.inner.test_origin.lock().unwrap() = Some(origin);
        let token = yt.seal("alice", "refresh-secret").unwrap();
        yt.db()
            .unwrap()
            .execute(
                "INSERT INTO youtube_connections VALUES (?1,?2,?3,?4)",
                params!["alice", token, "channel", "Channel"],
            )
            .unwrap();
        yt.db().unwrap().execute("INSERT INTO youtube_uploads(id,username,project_id,render,title,description,privacy,made_for_kids,state,total,channel_id) VALUES(?1,?2,?3,1,'Title','','private',0,'queued',?4,'channel')",params!["job","alice",project_id,8*1024*1024+3]).unwrap();
        assert!(upload_status(&yt, "bob", "job").is_err());
        run_upload(&yt, &store, "alice", "job").await.unwrap();
        let job = upload_status(&yt, "alice", "job").unwrap();
        assert_eq!(job.state, "complete");
        assert_eq!(job.video_id.as_deref(), Some("video123"));
        assert_eq!(job.actual_privacy.as_deref(), Some("private"));
        let ranges = server.join().unwrap();
        assert!(
            ranges
                .iter()
                .any(|r| r == "content-range: bytes 8388608-8388610/8388611")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn secrets_are_user_bound_and_sessions_must_be_google_https() {
        let root = std::env::temp_dir().join(format!("flicksy-youtube-test-{}", random()));
        std::fs::create_dir_all(&root).unwrap();
        let database = root.join("db.sqlite");
        let accounts = crate::accounts::Accounts::open(&database, false).unwrap();
        let _store = crate::store::Store::open(root.join("projects"), database.clone()).unwrap();
        accounts.add("alice", "test-password").unwrap();
        accounts.add("bob", "test-password").unwrap();
        std::fs::write(root.join("secret"), "secret").unwrap();
        std::fs::write(root.join("key"), URL_SAFE_NO_PAD.encode([23u8; 32])).unwrap();
        let yt = YouTube::new(
            Some(YouTubeConfig {
                client_id: "id".into(),
                client_secret_file: root.join("secret"),
                encryption_key_file: root.join("key"),
                redirect_uri: "https://example.com/youtube/callback".into(),
            }),
            database,
        )
        .unwrap();
        let encrypted = yt.seal("alice", "refresh-secret").unwrap();
        assert!(!encrypted.windows(14).any(|x| x == b"refresh-secret"));
        assert_eq!(yt.open("alice", &encrypted).unwrap(), "refresh-secret");
        assert!(yt.open("bob", &encrypted).is_err());
        assert_ne!(encrypted, yt.seal("alice", "refresh-secret").unwrap());
        assert!(valid_session_url(
            "https://www.googleapis.com/upload/youtube/v3/videos?upload_id=abc"
        ));
        assert!(!valid_session_url("http://www.googleapis.com/upload"));
        assert!(!valid_session_url(
            "https://www.googleapis.com.evil.example/upload"
        ));
        assert!(!valid_session_url(
            "https://evil.example/@www.googleapis.com"
        ));
        let session = accounts.login("alice", "test-password").unwrap().unwrap();
        let fingerprint = URL_SAFE_NO_PAD.encode(Sha256::digest(session.as_bytes()));
        assert!(
            !yt.save_connection("bob", &fingerprint, &encrypted, "channel", "Channel")
                .unwrap()
        );
        accounts.reset_password("alice", "new-password").unwrap();
        assert!(
            !yt.save_connection("alice", &fingerprint, &encrypted, "channel", "Channel")
                .unwrap()
        );
        let session = accounts.login("alice", "new-password").unwrap().unwrap();
        let fingerprint = URL_SAFE_NO_PAD.encode(Sha256::digest(session.as_bytes()));
        assert!(
            yt.save_connection("alice", &fingerprint, &encrypted, "channel", "Channel")
                .unwrap()
        );
        assert!(yt.connection("alice").unwrap().is_some());
        assert!(yt.connection("bob").unwrap().is_none());
        accounts.remove("alice").unwrap();
        assert!(yt.connection("alice").unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
