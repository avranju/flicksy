# Flicksy

Flicksy turns music and photos into videos. Copy `config.example.toml` to
`config.toml` and configure the LLM endpoint before starting the server.

## User accounts

Create an account before opening the app:

```sh
cargo run -- --config config.toml users add alice
cargo run -- --config config.toml users list
cargo run -- --config config.toml users remove alice
```

The add command prompts for a password without echoing it. Removing an account
also revokes its sessions. Commands can run while the server is running.

Accounts and sessions are stored in `flicksy.sqlite` in `projects_dir` by
default. Set `[auth] database` to choose a different path; relative paths are
resolved relative to the configuration file. Include the database in backups.
When Flicksy is accessed over HTTPS, set `[auth] secure_cookie = true`.

Sessions do not expire on the server, and the browser cookie is renewed during
use. Browsers can still discard cookies after extended inactivity or when their
storage is cleared. Use **Log out** to revoke the current session.

See [docker/README.md](docker/README.md) for container setup.
