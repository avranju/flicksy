# Running Flicksy in Docker

The runtime image includes FFmpeg, FFprobe, ImageMagick, Python, jq, and a shell.

Build the image from the project root:

```sh
docker build -t flicksy .
```

Copy `docker/config.toml` to `flicksy-container.toml` (which is excluded from Git
and the Docker build context). Replace the LLM endpoint/model placeholders,
set the optional API key source, and set `[operator] contact_email` to the public
contact address for this installation. That address is used at runtime on the
privacy policy and terms pages; restart the server after changing it. `localhost` inside the container
refers to the container itself. Keep the server host as `0.0.0.0` if using
Docker port publishing. The container config file is mounted read-only; the
project directory must be persistent and writable by UID/GID 10001.

```sh
docker volume create flicksy-projects
docker run --rm -p 127.0.0.1:3000:3000 \
  -v flicksy-projects:/var/lib/flicksy/projects \
  -v "$PWD/flicksy-container.toml:/etc/flicksy/config.toml:ro" \
  -e FLICKSY_API_KEY \
  flicksy
```

For a bind mount, create the host directory and arrange ownership or permissions
for UID/GID 10001. Docker initializes a new named volume with the ownership of
the image directory. Configure `api_key_env` to use the environment variable,
or mount an API key file and set `api_key_file`; omit both for an unauthenticated
endpoint. Keep credentials out of the image and build context.

The agent runs arbitrary LLM-generated shell commands with the container user's
privileges. Run only with trusted users and inputs in a dedicated environment;
the container does not isolate individual render runs. User accounts do not
prevent a render from accessing another user's files, the account database, or
credentials available to the container user.
Use one server process per project volume. Cancelling a render stops the agent
loop, but a command already launched by it may continue running.

The server writes structured tracing events to stderr. By default Flicksy logs
its own events at `info`. Set `[tracing] level = "debug"` in the configuration
for more detail. `RUST_LOG`, when set to a valid filter, overrides this setting;
for example, add `-e RUST_LOG=flicksy=debug` to `docker run`. Render-status polling
is suppressed at the default level. Prompts, credentials, shell commands, and
command output are not logged by Flicksy; enabling third-party trace targets may
produce additional diagnostics.

## User accounts

Create an account before signing in. Run this command against the same volume
and configuration as the server (replace the user name):

```sh
docker run --rm -it \
  -v flicksy-projects:/var/lib/flicksy/projects \
  -v "$PWD/flicksy-container.toml:/etc/flicksy/config.toml:ro" \
  flicksy --config /etc/flicksy/config.toml users add alice
```

Use `users reset-password alice` to set a new password and revoke its sessions,
`users list` to list accounts and `users remove alice` to remove one and
revoke its sessions. The password prompt does not echo input. Locally, use
`cargo run -- --config config.toml users add alice` (and the corresponding
`list` or `remove` command). The authentication database defaults to
`flicksy.sqlite` inside `projects_dir`; it also stores project records. Back up
the database and project directory together with the server stopped. Legacy
projects require the separate ownership migration described in the main README;
they are not imported automatically at startup.
Set `[auth] secure_cookie = true` when accessing Flicksy over HTTPS. Sessions
have no server-side expiry; browsers may discard cookies after long inactivity
or when their storage is cleared.

## Optional image-publishing workflow

The Gitea workflow in `.gitea/workflows/docker-image.yml` is manually triggered
and is not needed to build or run Flicksy locally. Before using it, set these
repository variables and secret for your own registry:

- `REGISTRY`: registry host, for example `registry.example.com` (no URL scheme).
- `REGISTRY_USERNAME`: your registry login name (`YOUR_REGISTRY_USERNAME`).
- `IMAGE_NAME`: image namespace and name, for example `YOUR_NAMESPACE/flicksy`.
- Secret `PACKAGE_TOKEN`: a credential authorized to push that image.

The workflow fails early if configuration is missing. Building privately for
your own use is different from publishing an image for others: a public image
redistributes FFmpeg and the other included packages, whose licenses and any
corresponding-source obligations must be addressed separately from Flicksy's MIT
license.

## YouTube uploads

YouTube integration is optional. In Google Cloud, enable the YouTube Data API
v3, configure the OAuth consent screen, and create a web application OAuth
client. Register the exact HTTPS callback URL ending in `/youtube/callback`.
For local development only, an HTTP localhost callback is also supported. Add
`[youtube]` to the Flicksy configuration using the fields documented in
`config.example.toml`. The client secret file contains the plain client secret,
not the Google client JSON document. The encryption key file contains an
unpadded base64url-encoded 32-byte random key, for example generated with
`python3 -c 'import secrets; print(secrets.token_urlsafe(32))'`.

Keep both files outside the repository and image; mount them read-only and make
sure UID 10001 can read them. Back up the encryption key separately and securely:
losing or changing it makes existing connections and in-progress sessions
unrecoverable. Google may require test users, OAuth verification or an API audit;
confirm the actual channel and visibility behavior for your Cloud project.

Each Flicksy user connects a YouTube channel from a render's upload screen.
The app requests upload and read-only YouTube scopes, stores encrypted refresh
tokens and resumable-session URLs in the account database, and displays saved
video links per render. Uploaded videos may need additional processing on
YouTube. On restart, return to the render's upload page to resume an interrupted
upload. If a session has expired or its result is uncertain, check YouTube Studio
before abandoning it and starting a new upload to avoid duplicates.

### Disconnecting and deleting YouTube data

Use **YouTube settings → Disconnect and delete YouTube data** in the account menu. This:

- Stops the account's active upload and invalidates pending OAuth flows.
- Immediately deletes its channel details, complete/interrupted/abandoned upload
  history, saved YouTube links, and resumable-upload sessions in one transaction.
- Sends the refresh token to Google's revocation endpoint in a POST body, not in
  the URL. Projects, source media, local renders, and videos already received by
  YouTube are left untouched. Use YouTube Studio to delete published videos.

If Google cannot confirm revocation, settings show a pending status and offer a
retry button. Only the encrypted credential, account identifier (used for
credential decryption), and request time remain in a durable revocation queue.
The server retries at startup and hourly, and deletes the credential after seven
days even if Google is still unavailable. Reconnection is blocked while pending;
you can always revoke access directly at
[Google account permissions](https://myaccount.google.com/permissions).
Disconnect before disabling the integration or replacing the encryption key:
Flicksy needs the original key to decrypt queued credentials for revocation.

While running, the server checks existing authorizations at startup and hourly.
A confirmed `invalid_grant` response removes the connection and local YouTube
records immediately. Network errors, rate limits, and client-configuration errors
do not cause immediate data loss. If an authorization cannot be verified for
30 days, its YouTube data is deleted after a final revocation attempt; the
credential is not retained for an additional retry period. These time limits
are evaluated at startup and on the hourly pass; no maintenance can run while
the server is stopped. Keep the service running or handle deletion/revocation
manually to meet applicable deadlines during extended downtime.

Removing an account with `users remove` queues its encrypted credential in the
same transaction as account deletion. The running server (or its next startup)
then attempts revocation; the queue survives account deletion and server
restarts. Existing project-ownership restrictions on account deletion still
apply. Legacy upload records with no remaining connection are removed by
maintenance. Existing connection records are migrated automatically and checked
at startup; if Google is temporarily unavailable, they receive a 30-day
verification grace period from migration.

Database deletion does not remove copies from backups or filesystem snapshots.
Operators must remove affected backup data within the applicable policy deadlines
and must not restore deleted YouTube data into service. Check Google permissions
after restoring a backup or after an unconfirmed revocation. Hourly token checks
and disconnect cleanup are not a substitute for reviewing the rest of YouTube's
API policies for your installation.
