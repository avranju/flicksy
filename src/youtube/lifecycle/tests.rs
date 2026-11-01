use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Fixture {
    root: PathBuf,
    yt: YouTube,
    store: Arc<Store>,
    accounts: crate::accounts::Accounts,
    project: String,
}
impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("flicksy-consent-test-{}", random()));
        std::fs::create_dir_all(&root).unwrap();
        let database = root.join("db.sqlite");
        let accounts = crate::accounts::Accounts::open(&database, false).unwrap();
        let db = Connection::open(&database).unwrap();
        db.execute_batch("INSERT INTO users VALUES ('alice','test-hash'),('bob','test-hash'); INSERT INTO sessions VALUES ('alice-session','alice','test-hash'),('bob-session','bob','test-hash');").unwrap();
        let store = Arc::new(Store::open(root.join("projects"), database.clone()).unwrap());
        std::fs::write(root.join("secret"), "test-client-secret").unwrap();
        std::fs::write(root.join("key"), URL_SAFE_NO_PAD.encode([7u8; 32])).unwrap();
        let yt = YouTube::new(
            Some(YouTubeConfig {
                client_id: "test-client".into(),
                client_secret_file: root.join("secret"),
                encryption_key_file: root.join("key"),
                redirect_uri: "https://example.com/youtube/callback".into(),
            }),
            database,
        )
        .unwrap();
        let mut project = String::new();
        for user in ["alice", "bob"] {
            let pending = store.begin().unwrap();
            std::fs::write(pending.path.join("video.mp4"), b"test-render").unwrap();
            std::fs::write(pending.path.join("original.wav"), b"test-audio").unwrap();
            let id = store
                .commit(
                    pending,
                    user,
                    &crate::store::Project {
                        title: format!("{user}'s project"),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            store.record_render(user, &id, "video.mp4").await.unwrap();
            let token = yt.seal(user, &format!("test-refresh-{user}")).unwrap();
            assert!(
                yt.save_connection(
                    user,
                    &format!("{user}-session"),
                    &token,
                    &format!("channel-{user}"),
                    "Channel"
                )
                .unwrap()
            );
            for state in ["complete", "interrupted", "abandoned"] {
                yt.db().unwrap().execute(
                    "INSERT INTO youtube_uploads (id,username,project_id,render,title,description,privacy,made_for_kids,state,session,total,channel_id,video_id) VALUES (?1,?2,?3,1,'Title','Description','private',0,?4,?5,11,?6,'video-id')",
                    params![format!("{user}-{state}"), user, id, state, yt.seal(user, "https://www.googleapis.com/session").unwrap(), format!("channel-{user}")],
                ).unwrap();
            }
            if user == "alice" {
                project = id;
            }
        }
        Self {
            root,
            yt,
            store,
            accounts,
            project,
        }
    }

    fn router(&self) -> topcoat::router::Router {
        topcoat::router::Router::builder()
            .route(callback)
            .route(disconnect)
            .route(connect)
            .page(settings)
            .app_context(self.yt.clone())
            .app_context(self.store.clone())
            .build()
    }

    fn prepare_oauth(&self) -> String {
        self.yt.erase_youtube_data("alice", None, false).unwrap();
        let cookie = "A".repeat(43);
        let session = URL_SAFE_NO_PAD.encode(Sha256::digest(cookie.as_bytes()));
        self.yt
            .db()
            .unwrap()
            .execute(
                "INSERT INTO sessions VALUES (?1,'alice','test-hash')",
                [&session],
            )
            .unwrap();
        let generation = *self
            .yt
            .inner
            .connection_generation
            .lock()
            .unwrap()
            .get("alice")
            .unwrap();
        self.yt.inner.flows.lock().unwrap().insert(
            "oauth-state".into(),
            Flow {
                user: "alice".into(),
                session,
                verifier: "test-verifier".into(),
                project: self.project.clone(),
                render: 1,
                created: Instant::now(),
                generation,
            },
        );
        cookie
    }

    fn count(&self, table: &str, user: &str) -> i64 {
        self.yt
            .db()
            .unwrap()
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE username=?1"),
                [user],
                |r| r.get(0),
            )
            .unwrap()
    }

    async fn assert_alice_erased(&self) {
        assert!(self.yt.connection("alice").unwrap().is_none());
        assert_eq!(self.count("youtube_uploads", "alice"), 0);
        assert!(self.yt.connection("bob").unwrap().is_some());
        assert_eq!(self.count("youtube_uploads", "bob"), 3);
        assert_eq!(
            self.store
                .load_project("alice", &self.project)
                .await
                .unwrap()
                .renders
                .len(),
            1
        );
        assert_eq!(
            std::fs::read(self.store.project_path(&self.project).join("video.mp4")).unwrap(),
            b"test-render"
        );
        assert_eq!(
            std::fs::read(self.store.project_path(&self.project).join("original.wav")).unwrap(),
            b"test-audio"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Reply {
    path: &'static str,
    status: u16,
    body: &'static str,
    before_response: Option<Box<dyn FnOnce() + Send>>,
}
impl Reply {
    fn new(path: &'static str, status: u16, body: &'static str) -> Self {
        Self {
            path,
            status,
            body,
            before_response: None,
        }
    }
}
struct FakeGoogle {
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl FakeGoogle {
    async fn start(yt: &YouTube, replies: Vec<Reply>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        *yt.inner.test_origin.lock().unwrap() =
            Some(format!("http://{}", listener.local_addr().unwrap()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            for reply in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let end = loop {
                    let mut buf = [0u8; 2048];
                    let n = stream.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(p) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .and_then(|s| s.parse().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() - end < length {
                    let mut buf = [0u8; 2048];
                    let n = stream.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                let request = String::from_utf8(bytes).unwrap();
                if reply.path == "/channels" {
                    assert!(request.starts_with("GET /channels?"));
                } else {
                    assert!(request.starts_with(&format!("POST {} HTTP/1.1", reply.path)));
                }
                captured.lock().unwrap().push(request);
                if let Some(before_response) = reply.before_response {
                    before_response();
                }
                let response = format!(
                    "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.status,
                    reply.body.len(),
                    reply.body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self { requests, task }
    }
    async fn finish(self) -> Vec<String> {
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap();
        self.requests.lock().unwrap().clone()
    }
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn disconnect_revokes_purges_all_states_and_cancels_upload_without_touching_media() {
    let f = Fixture::new().await;
    let server = FakeGoogle::start(&f.yt, vec![Reply::new("/revoke", 200, "")]).await;
    let registration = UploadRegistration {
        youtube: f.yt.clone(),
        user: "alice".into(),
        id: "alice-interrupted".into(),
    };
    let task = tokio::spawn(async move {
        let _registration = registration;
        std::future::pending::<()>().await;
    });
    f.yt.inner.active.lock().unwrap().insert(
        "alice".into(),
        ActiveUpload {
            id: "alice-interrupted".into(),
            task: task.abort_handle(),
        },
    );
    f.yt.inner.flows.lock().unwrap().insert(
        "oauth-state".into(),
        Flow {
            user: "alice".into(),
            session: "alice-session".into(),
            verifier: "test".into(),
            project: f.project.clone(),
            render: 1,
            created: Instant::now(),
            generation: 0,
        },
    );
    assert!(f.yt.disconnect_user("alice").await.unwrap());
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(f.yt.inner.flows.lock().unwrap().is_empty());
    assert_eq!(
        f.yt.inner
            .connection_generation
            .lock()
            .unwrap()
            .get("alice"),
        Some(&1)
    );
    assert!(!f.yt.revocation_pending("alice").unwrap());
    f.assert_alice_erased().await;
    let requests = server.finish().await;
    assert!(requests[0].contains("token=test-refresh-alice"));
    assert!(!requests[0].lines().next().unwrap().contains("token="));
    assert!(f.yt.disconnect_user("alice").await.unwrap()); // Idempotent, no further request.
    assert!(
        launch(
            &f.yt,
            f.store.clone(),
            "alice".into(),
            "alice-interrupted".into()
        )
        .is_err()
    );
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn failed_revocation_is_encrypted_durable_and_blocks_reconnection_until_retry() {
    let f = Fixture::new().await;
    let server = FakeGoogle::start(
        &f.yt,
        vec![
            Reply::new("/revoke", 503, "{}"),
            Reply::new("/revoke", 200, ""),
        ],
    )
    .await;
    assert!(!f.yt.disconnect_user("alice").await.unwrap());
    f.assert_alice_erased().await;
    let pending: Vec<u8> =
        f.yt.db()
            .unwrap()
            .query_row(
                "SELECT token FROM youtube_revocations WHERE username='alice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert!(
        !pending
            .windows(b"test-refresh-alice".len())
            .any(|s| s == b"test-refresh-alice")
    );
    let new_token = f.yt.seal("alice", "new-refresh").unwrap();
    assert!(
        !f.yt
            .save_connection("alice", "alice-session", &new_token, "new-channel", "New")
            .unwrap()
    );
    let restarted = YouTube::new(f.yt.inner.config.clone(), f.yt.inner.database.clone()).unwrap();
    *restarted.inner.test_origin.lock().unwrap() = f.yt.inner.test_origin.lock().unwrap().clone();
    assert!(restarted.revocation_pending("alice").unwrap());
    assert!(restarted.disconnect_user("alice").await.unwrap());
    assert!(!restarted.revocation_pending("alice").unwrap());
    assert!(
        restarted
            .save_connection("alice", "alice-session", &new_token, "new-channel", "New")
            .unwrap()
    );
    assert_eq!(server.finish().await.len(), 2);
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn revocation_distinguishes_invalid_tokens_from_other_bad_requests() {
    let f = Fixture::new().await;
    let server = FakeGoogle::start(
        &f.yt,
        vec![
            Reply::new("/revoke", 400, "{\"error\":\"invalid_request\"}"),
            Reply::new("/revoke", 400, "{\"error\":\"invalid_token\"}"),
        ],
    )
    .await;
    assert!(!f.yt.disconnect_user("alice").await.unwrap());
    assert!(f.yt.revocation_pending("alice").unwrap());
    assert!(f.yt.disconnect_user("alice").await.unwrap());
    assert!(!f.yt.revocation_pending("alice").unwrap());
    server.finish().await;
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn maintenance_detects_external_revocation_without_deleting_other_users_or_media() {
    let f = Fixture::new().await;
    // Maintenance processes account names in a stable order.
    f.yt.db()
        .unwrap()
        .execute(
            "UPDATE youtube_connections SET last_verified_at=unixepoch()",
            [],
        )
        .unwrap();
    let server = FakeGoogle::start(
        &f.yt,
        vec![
            Reply::new("/token", 400, "{\"error\":\"invalid_grant\"}"),
            Reply::new("/token", 200, "{\"access_token\":\"test-access\"}"),
        ],
    )
    .await;
    f.yt.maintain_once().await.unwrap();
    f.assert_alice_erased().await;
    assert!(!f.yt.revocation_pending("alice").unwrap());
    server.finish().await;
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn transient_and_client_errors_do_not_delete_valid_connections() {
    let f = Fixture::new().await;
    let server = FakeGoogle::start(
        &f.yt,
        vec![
            Reply::new("/token", 503, "{}"),
            Reply::new("/token", 400, "{\"error\":\"invalid_client\"}"),
            Reply::new("/token", 200, "not-json"),
            Reply::new("/token", 200, "{}"),
            Reply::new("/token", 200, "{\"access_token\":\"test-access\"}"),
        ],
    )
    .await;
    for _ in 0..4 {
        assert!(f.yt.access_token("alice").await.is_err());
        assert!(f.yt.connection("alice").unwrap().is_some());
        assert_eq!(f.count("youtube_uploads", "alice"), 3);
    }
    assert_eq!(f.yt.access_token("alice").await.unwrap(), "test-access");
    server.finish().await;
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn stale_refresh_response_cannot_erase_a_new_connection() {
    let f = Fixture::new().await;
    let yt = f.yt.clone();
    let mut reply = Reply::new("/token", 400, "{\"error\":\"invalid_grant\"}");
    reply.before_response = Some(Box::new(move || {
        let replacement = yt.seal("alice", "replacement-token").unwrap();
        assert!(
            yt.save_connection(
                "alice",
                "alice-session",
                &replacement,
                "replacement-channel",
                "New"
            )
            .unwrap()
        );
    }));
    let server = FakeGoogle::start(&f.yt, vec![reply]).await;
    assert!(f.yt.access_token("alice").await.is_err());
    assert_eq!(
        f.yt.connection("alice").unwrap().unwrap().1,
        "replacement-channel"
    );
    assert_eq!(f.count("youtube_uploads", "alice"), 3);
    server.finish().await;
}

#[tokio::test]
async fn expired_revocations_are_erased_and_failed_account_deletion_does_not_revoke() {
    let f = Fixture::new().await;
    assert!(f.accounts.remove("alice").is_err()); // Projects prohibit deletion.
    assert!(!f.yt.revocation_pending("alice").unwrap());
    f.yt.erase_youtube_data("alice", None, true).unwrap();
    f.yt.db()
        .unwrap()
        .execute(
            "UPDATE youtube_revocations SET requested_at=?1",
            [now() - REVOCATION_RETENTION],
        )
        .unwrap();
    assert!(!f.yt.disconnect_user("alice").await.unwrap()); // No network required.
    assert!(!f.yt.revocation_pending("alice").unwrap());
    f.assert_alice_erased().await;
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn concurrent_disconnects_send_only_one_revocation_request() {
    let f = Fixture::new().await;
    let server = FakeGoogle::start(&f.yt, vec![Reply::new("/revoke", 200, "")]).await;
    let (a, b) = tokio::join!(f.yt.disconnect_user("alice"), f.yt.disconnect_user("alice"));
    assert!(a.unwrap());
    assert!(b.unwrap());
    assert_eq!(server.finish().await.len(), 1);
    f.assert_alice_erased().await;
}

#[tokio::test]
async fn unverified_data_expires_even_if_integration_is_disabled() {
    let f = Fixture::new().await;
    f.yt.db()
        .unwrap()
        .execute(
            "UPDATE youtube_connections SET last_verified_at=?1 WHERE username='alice'",
            [now() - AUTHORIZATION_RETENTION],
        )
        .unwrap();
    let disabled = YouTube::new(None, f.yt.inner.database.clone()).unwrap();
    disabled.maintain_once().await.unwrap();
    f.assert_alice_erased().await;
    assert!(!f.yt.revocation_pending("alice").unwrap());
}

#[tokio::test]
async fn cleanup_is_atomic_if_a_database_write_fails() {
    let f = Fixture::new().await;
    f.yt.db().unwrap().execute_batch("CREATE TRIGGER fail_youtube_delete BEFORE DELETE ON youtube_uploads BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
    assert!(f.yt.erase_youtube_data("alice", None, true).is_err());
    assert!(f.yt.connection("alice").unwrap().is_some());
    assert_eq!(f.count("youtube_uploads", "alice"), 3);
    assert!(!f.yt.revocation_pending("alice").unwrap());
}

#[tokio::test]
async fn legacy_connection_schema_migrates_without_discarding_data() {
    let f = Fixture::new().await;
    f.yt.db()
        .unwrap()
        .execute_batch("ALTER TABLE youtube_connections DROP COLUMN last_verified_at")
        .unwrap();
    let migrated = YouTube::new(f.yt.inner.config.clone(), f.yt.inner.database.clone()).unwrap();
    assert!(migrated.connection("alice").unwrap().is_some());
    assert_eq!(f.count("youtube_uploads", "alice"), 3);
    let verified: i64 = migrated
        .db()
        .unwrap()
        .query_row(
            "SELECT last_verified_at FROM youtube_connections WHERE username='alice'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(verified > now() - 5);
}

#[tokio::test]
async fn old_upload_registration_cannot_remove_a_new_jobs_registration() {
    let f = Fixture::new().await;
    let old = UploadRegistration {
        youtube: f.yt.clone(),
        user: "alice".into(),
        id: "old-job".into(),
    };
    let new = tokio::spawn(std::future::pending::<()>());
    f.yt.inner.active.lock().unwrap().insert(
        "alice".into(),
        ActiveUpload {
            id: "new-job".into(),
            task: new.abort_handle(),
        },
    );
    drop(old);
    assert_eq!(
        f.yt.inner.active.lock().unwrap().get("alice").unwrap().id,
        "new-job"
    );
    f.yt.erase_youtube_data("alice", None, true).unwrap();
    assert!(new.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn disconnect_invalidates_old_oauth_callback_before_any_google_request() {
    let f = Fixture::new().await;
    let cookie = f.prepare_oauth();
    assert!(f.yt.disconnect_user("alice").await.unwrap()); // No stored token.
    let request = http::Request::builder()
        .uri("https://example.com/youtube/callback?state=oauth-state&code=test-code")
        .header("Cookie", format!("flicksy_session={cookie}"))
        .body(Body::empty())
        .unwrap();
    let response = f
        .router()
        .handle_with(request, (auth::SignedInUser("alice".into()),))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    f.assert_alice_erased().await;
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn failed_oauth_channel_lookup_revokes_the_unused_refresh_token() {
    let f = Fixture::new().await;
    let cookie = f.prepare_oauth();
    let server = FakeGoogle::start(
        &f.yt,
        vec![
            Reply::new(
                "/token",
                200,
                "{\"access_token\":\"test-access\",\"refresh_token\":\"new-test-refresh\"}",
            ),
            Reply::new("/channels", 503, "{}"),
            Reply::new("/revoke", 200, ""),
        ],
    )
    .await;
    let request = http::Request::builder()
        .uri("https://example.com/youtube/callback?state=oauth-state&code=test-code")
        .header("Cookie", format!("flicksy_session={cookie}"))
        .body(Body::empty())
        .unwrap();
    let response = f
        .router()
        .handle_with(request, (auth::SignedInUser("alice".into()),))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(!f.yt.revocation_pending("alice").unwrap());
    f.assert_alice_erased().await;
    assert!(server.finish().await[2].contains("token=new-test-refresh"));
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn startup_worker_retries_pending_revocations_without_user_activity() {
    let f = Fixture::new().await;
    f.yt.erase_youtube_data("alice", None, true).unwrap();
    f.yt.db()
        .unwrap()
        .execute(
            "UPDATE youtube_connections SET last_verified_at=unixepoch()-10",
            [],
        )
        .unwrap();
    let server = FakeGoogle::start(
        &f.yt,
        vec![
            Reply::new("/revoke", 200, ""),
            Reply::new("/token", 200, "{\"access_token\":\"test-access\"}"),
        ],
    )
    .await;
    let worker = f.yt.start_maintenance();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let verified: i64 =
                f.yt.db()
                    .unwrap()
                    .query_row(
                        "SELECT last_verified_at FROM youtube_connections WHERE username='bob'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
            if !f.yt.revocation_pending("alice").unwrap() && verified >= now() - 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    worker.abort();
    assert_eq!(server.finish().await.len(), 2);
    f.assert_alice_erased().await;
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn malformed_oauth_success_does_not_leak_an_issued_refresh_token() {
    let f = Fixture::new().await;
    let cookie = f.prepare_oauth();
    let server = FakeGoogle::start(
        &f.yt,
        vec![
            Reply::new("/token", 200, "{\"refresh_token\":\"new-test-refresh\"}"),
            Reply::new("/revoke", 200, ""),
        ],
    )
    .await;
    let request = http::Request::builder()
        .uri("https://example.com/youtube/callback?state=oauth-state&code=test-code")
        .header("Cookie", format!("flicksy_session={cookie}"))
        .body(Body::empty())
        .unwrap();
    let response = f
        .router()
        .handle_with(request, (auth::SignedInUser("alice".into()),))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(!f.yt.revocation_pending("alice").unwrap());
    assert!(server.finish().await[1].contains("token=new-test-refresh"));
}

#[tokio::test]
#[ignore = "requires permission to bind a loopback test server"]
async fn disconnect_route_reports_pending_and_settings_explain_deletion() {
    use http_body_util::BodyExt;
    let f = Fixture::new().await;
    let server = FakeGoogle::start(&f.yt, vec![Reply::new("/revoke", 503, "{}")]).await;
    let router = f.router();
    let request = http::Request::builder()
        .method("POST")
        .uri("https://example.com/youtube/disconnect")
        .body(Body::empty())
        .unwrap();
    let response = router
        .handle_with(request, (auth::SignedInUser("alice".into()),))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers()["location"],
        "/youtube/settings?disconnected=pending"
    );
    let request = http::Request::builder()
        .uri("https://example.com/youtube/settings?disconnected=pending")
        .body(Body::empty())
        .unwrap();
    let response = router
        .handle_with(request, (auth::SignedInUser("alice".into()),))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("Retry Google revocation"));
    assert!(html.contains("Reconnecting is blocked"));
    assert!(!html.contains("Google authorization was revoked"));
    server.finish().await;
}

#[tokio::test]
async fn account_removal_keeps_only_the_encrypted_revocation_credential() {
    let f = Fixture::new().await;
    // Remove this user's project records to permit account deletion (not a media deletion).
    f.yt.db()
        .unwrap()
        .execute("DELETE FROM youtube_uploads WHERE username='alice'", [])
        .unwrap();
    f.yt.db()
        .unwrap()
        .execute("DELETE FROM projects WHERE username='alice'", [])
        .unwrap();
    f.accounts.remove("alice").unwrap();
    assert!(f.yt.connection("alice").unwrap().is_none());
    assert!(f.yt.revocation_pending("alice").unwrap());
    let token: Vec<u8> =
        f.yt.db()
            .unwrap()
            .query_row(
                "SELECT token FROM youtube_revocations WHERE username='alice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(f.yt.open("alice", &token).unwrap(), "test-refresh-alice");
}
