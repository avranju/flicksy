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
also revokes its sessions. An account that owns projects cannot be removed.
Commands can run while the server is running.

Accounts, sessions, and project records are stored in `flicksy.sqlite` in
`projects_dir` by default. Set `[auth] database` to choose a different path; relative paths are
resolved relative to the configuration file. Include the database in backups.
When Flicksy is accessed over HTTPS, set `[auth] secure_cookie = true`.

Sessions do not expire on the server, and the browser cookie is renewed during
use. Browsers can still discard cookies after extended inactivity or when their
storage is cleared. Use **Log out** to revoke the current session.

## Project storage and migration

Each project belongs to its creator; project lists, editing, renders, and media
are restricted to that account. The database enforces ownership and prevents
deleting an account that still owns projects.

Existing projects require a separate, one-time migration. Stop Flicksy, back up
its database and project directory, and create the destination user accounts.
Create a JSON file mapping every existing project ID to its owner's username:

```json
{
  "project-123": "alice",
  "project-456": "bob"
}
```

Run the migration from the project checkout, adjusting paths as needed:

```sh
python3 scripts/migrate_projects.py \
  --database projects/flicksy.sqlite \
  --projects-dir projects \
  --owners project-owners.json
```

Use the configured `[auth] database` path if it differs from the default. The
script imports the old `index.json` and `project.json` files. It requires
exactly one existing user per project, performs the change in one database transaction,
and leaves the original JSON and media untouched. It refuses to run again after
ownership has been assigned. Normal server startup never imports these files.

Back up the SQLite database and the entire projects directory together. Stop the
server while taking a filesystem copy so the database and media represent the
same state. If `[auth] database` points outside `projects_dir`, include that
file separately. The old JSON files alone do not reflect later project changes.

See [docker/README.md](docker/README.md) for container setup.
