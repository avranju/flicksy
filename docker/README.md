# Running Flicksy in Docker

The runtime image includes FFmpeg, FFprobe, ImageMagick, Python, jq, and a shell.

Build the image from the project root:

```sh
docker build -t flicksy .
```

Copy `docker/config.toml` to a deployment-specific file and update the LLM
endpoint, model, and optional API key source. `localhost` inside the container
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

The agent runs shell commands with the container user's privileges. Run only in
a trusted environment; the container does not isolate individual render runs.
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
YouTube. Use the account menu to disconnect; this removes the stored connection,
while Google account permissions can be revoked separately. On restart, return
to the render's upload page to resume an interrupted upload. If a session has
expired or its result is uncertain, check YouTube Studio before abandoning it
and starting a new upload to avoid duplicates.
