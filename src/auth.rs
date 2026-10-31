use topcoat::{
    Result,
    router::page,
    view::{View, view},
};

#[page("/login")]
async fn login(cx: &Cx) -> Result<impl View> {
    let failed = uri(cx).query().is_some_and(|q| q == "error=1");
    Ok(view! {
        <main class="mx-auto w-full max-w-[520px] px-6 pt-12 pb-16 sm:pt-20">
            <section aria-labelledby="login-title">
                <p class="text-xs font-bold text-accent-ink">"WELCOME TO FLICKSY"</p>
                <h1 id="login-title" class="mt-4 text-[30px] leading-[1.2] font-bold sm:text-[34px]">
                    "Welcome back"
                </h1>
                <p class="mt-4 text-base text-muted-foreground">
                    "Sign in to create videos and access your projects."
                </p>
                <form method="post" action="/login" class="mt-8 flex flex-col gap-6 rounded-[20px] bg-card px-5 py-9 sm:px-10">
                    <div class="flex flex-col gap-2">
                        <label for="username" class="text-sm font-semibold">"Username"</label>
                        <input id="username" name="username" type="text" autofocus="autofocus" autocomplete="username" required="required" class="w-full rounded-xl border border-border bg-surface px-4 py-3 text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8]">
                    </div>
                    <div class="flex flex-col gap-2">
                        <label for="password" class="text-sm font-semibold">"Password"</label>
                        <input id="password" name="password" type="password" autocomplete="current-password" required="required" class="w-full rounded-xl border border-border bg-surface px-4 py-3 text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8]">
                    </div>
                    if failed {
                        <p role="alert" class="text-sm text-danger-ink">"Unable to sign in. Check your credentials and try again."</p>
                    }
                    <button type="submit" class="cursor-pointer rounded-xl bg-[#6554e8] px-4 py-3 text-sm font-bold text-white transition motion-reduce:transition-none hover:bg-[#5544d4] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#6554e8] active:scale-[0.98] motion-reduce:active:scale-100">
                        "Sign in"
                    </button>
                </form>
            </section>
        </main>
    })
}

use crate::accounts::Accounts;
use http::{Method, StatusCode, header};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use topcoat::{
    context::{Cx, app_context},
    router::{
        Body, Layer, LayerFuture, Next, Path,
        content::Form,
        request::{headers, method, uri},
        response::Response,
        route,
    },
};

pub struct SignedInUser(pub String);

pub fn username(cx: &Cx) -> &str {
    &topcoat::context::request_context::<SignedInUser>(cx).0
}

const COOKIE: &str = "flicksy_session";
const MAX_AGE: u64 = 400 * 24 * 60 * 60;

#[derive(Clone)]
pub struct AuthLayer {
    pub accounts: Arc<Accounts>,
    attempts: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
    hashing: Arc<tokio::sync::Semaphore>,
}
impl AuthLayer {
    pub fn new(accounts: Arc<Accounts>) -> Self {
        Self {
            accounts,
            attempts: Default::default(),
            hashing: Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }
    fn admit(&self, username: &str) -> bool {
        if username.len() > 128 {
            return false;
        }
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        attempts.retain(|_, (when, _)| now.duration_since(*when) < Duration::from_secs(60));
        let global = attempts.entry(String::new()).or_insert((now, 0));
        if global.1 >= 60 {
            return false;
        }
        global.1 += 1;
        let account = attempts.entry(username.to_owned()).or_insert((now, 0));
        if account.1 >= 5 {
            return false;
        }
        account.1 += 1;
        true
    }
}
fn cookie(headers: &http::HeaderMap) -> Option<&str> {
    let mut tokens = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|p| p.trim().split_once('='))
        .filter(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value);
    let token = tokens.next()?;
    if tokens.next().is_some()
        || token.len() != 43
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    Some(token)
}
pub fn session_fingerprint(cx: &Cx) -> Option<String> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use sha2::{Digest, Sha256};
    cookie(headers(cx)).map(|token| URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes())))
}
fn set_cookie(response: &mut Response, accounts: &Accounts, value: &str, age: u64) {
    let secure = if accounts.secure_cookie {
        "; Secure"
    } else {
        ""
    };
    let value = format!("{COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{secure}");
    response
        .headers_mut()
        .append(header::SET_COOKIE, value.parse().expect("valid cookie"));
}
fn redirect(location: &str) -> Response {
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, location)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .unwrap()
}
fn same_origin(headers: &http::HeaderMap) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<http::Uri>().ok())
    else {
        return false;
    };
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    matches!(origin.scheme_str(), Some("http" | "https"))
        && origin
            .authority()
            .is_some_and(|a| a.as_str().eq_ignore_ascii_case(host))
        && (origin.path().is_empty() || origin.path() == "/")
        && origin.query().is_none()
}
impl Layer for AuthLayer {
    fn path(&self) -> Option<&Path> {
        None
    }
    fn handle<'a>(&'a self, cx: &'a Cx, body: Body, next: Next<'a>) -> LayerFuture<'a> {
        Box::pin(async move {
            let path = uri(cx).path();
            if !matches!(*method(cx), Method::GET | Method::HEAD | Method::OPTIONS)
                && !same_origin(headers(cx))
            {
                return Ok(Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Body::from("A same-origin request is required"))?);
            }
            if path == "/login"
                || (matches!(path, "/privacy" | "/terms")
                    && matches!(*method(cx), Method::GET | Method::HEAD))
                || path.starts_with("/_topcoat/")
            {
                return next.run(cx, body).await;
            }
            let token = cookie(headers(cx));
            let username = match token {
                Some(t) => self.accounts.session(t)?,
                None => None,
            };
            let Some(username) = username else {
                return Ok(redirect("/login"));
            };
            let user_context = cx.with(SignedInUser(username));
            let mut response = next.run(&user_context, body).await?;
            if path != "/logout" {
                set_cookie(&mut response, &self.accounts, token.unwrap(), MAX_AGE);
            }
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
            Ok(response)
        })
    }
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

#[route(POST "/login")]
async fn login_submit(cx: &Cx, Form(credentials): Form<Credentials>) -> Result<Response> {
    let auth = app_context::<AuthLayer>(cx).clone();
    if !auth.admit(&credentials.username) {
        return Ok(redirect("/login?error=1"));
    }
    let Ok(permit) = auth.hashing.clone().try_acquire_owned() else {
        return Ok(redirect("/login?error=1"));
    };
    let accounts = auth.accounts.clone();
    let token = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        accounts.login(&credentials.username, &credentials.password)
    })
    .await
    .map_err(std::io::Error::other)??;
    let Some(token) = token else {
        return Ok(redirect("/login?error=1"));
    };
    if let Some(previous) = cookie(headers(cx)) {
        auth.accounts.logout(previous)?;
    }
    let mut response = redirect("/");
    set_cookie(&mut response, &auth.accounts, &token, MAX_AGE);
    Ok(response)
}

#[route(POST "/logout")]
async fn logout(cx: &Cx) -> Result<Response> {
    let auth = app_context::<AuthLayer>(cx);
    if let Some(token) = cookie(headers(cx)) {
        auth.accounts.logout(token)?;
    }
    let mut response = redirect("/login");
    set_cookie(&mut response, &auth.accounts, "", 0);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_origin_and_rejects_ambiguous_cookies() {
        let mut headers = http::HeaderMap::new();
        headers.insert(header::HOST, "example.com".parse().unwrap());
        headers.insert(header::ORIGIN, "https://example.com".parse().unwrap());
        assert!(same_origin(&headers));
        headers.insert(header::ORIGIN, "https://attacker.example".parse().unwrap());
        assert!(!same_origin(&headers));
        let token = "A".repeat(43);
        headers.insert(header::COOKIE, format!("{COOKIE}={token}").parse().unwrap());
        assert_eq!(cookie(&headers), Some(token.as_str()));
        headers.append(header::COOKIE, format!("{COOKIE}={token}").parse().unwrap());
        assert!(cookie(&headers).is_none());
    }
}
