#!/usr/bin/env python3
"""One-time import of legacy projects, assigning an owner to each project."""
import argparse
import json
import pathlib
import sqlite3
import sys

SCHEMA = """CREATE TABLE projects (
    id TEXT PRIMARY KEY NOT NULL,
    username TEXT NOT NULL REFERENCES users(username) ON DELETE RESTRICT,
    title TEXT NOT NULL,
    song_json TEXT NOT NULL,
    images_json TEXT NOT NULL,
    image_number_high_water TEXT NOT NULL,
    credits_json TEXT NOT NULL,
    instructions TEXT NOT NULL,
    renders_json TEXT NOT NULL
)"""
COLUMNS = "id, title, song_json, images_json, image_number_high_water, credits_json, instructions, renders_json"


def validate_project(project, project_id):
    def u64(value):
        return isinstance(value, int) and not isinstance(value, bool) and 0 <= value <= 2**64 - 1

    def asset(value):
        return isinstance(value, dict) and isinstance(value.get("original_name"), str) and isinstance(value.get("path"), str)

    song = project.get("song")
    images = project.get("images")
    credits = project.get("credits")
    renders = project.get("renders", [])
    valid = (isinstance(project.get("title"), str) and (song is None or asset(song))
             and isinstance(images, list) and all(isinstance(image, dict) and u64(image.get("number")) and asset(image.get("asset")) for image in images)
             and u64(project.get("image_number_high_water", 0))
             and isinstance(credits, list) and all(isinstance(credit, str) for credit in credits)
             and isinstance(project.get("instructions"), str)
             and isinstance(renders, list) and all(isinstance(render, dict) and u64(render.get("number")) and isinstance(render.get("path"), str) for render in renders))
    if not valid or "song" not in project:
        raise ValueError(f"Invalid project metadata for {project_id}")


def read_legacy(root):
    index = json.loads((root / "index.json").read_text(encoding="utf-8"))
    result = []
    for item in index["projects"]:
        project_id = item["id"]
        if (not isinstance(project_id, str) or not project_id or project_id in (".", "..")
                or "/" in project_id or "\\" in project_id or item["path"] != project_id):
            raise ValueError("Invalid project ID or path in index.json")
        project = json.loads((root / project_id / "project.json").read_text(encoding="utf-8"))
        if not isinstance(project, dict):
            raise ValueError(f"Invalid project metadata for {project_id}")
        validate_project(project, project_id)
        high_water = project.get("image_number_high_water", 0)
        if not isinstance(high_water, int) or isinstance(high_water, bool) or not 0 <= high_water <= 2**64 - 1:
            raise ValueError(f"Invalid image high-water mark for {project_id}")
        encode = lambda value: json.dumps(value, ensure_ascii=False, separators=(",", ":"))
        result.append((project_id, project["title"], encode(project["song"]), encode(project["images"]),
                       str(high_water), encode(project["credits"]), project["instructions"],
                       encode(project.get("renders", []))))
    return result


def migrate(database, root, assignments):
    if not database.is_file():
        raise ValueError("Database does not exist; create the user accounts first")
    owners = json.loads(assignments.read_text(encoding="utf-8"))
    if not isinstance(owners, dict) or any(not isinstance(k, str) or not isinstance(v, str) or not v for k, v in owners.items()):
        raise ValueError("Owner mapping must be a JSON object mapping project IDs to usernames")
    connection = sqlite3.connect(database, timeout=5, isolation_level=None)
    try:
        connection.execute("PRAGMA foreign_keys=ON")
        connection.execute("BEGIN IMMEDIATE")
        exists = connection.execute("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='projects')").fetchone()[0]
        if exists:
            raise ValueError("Projects table already exists; refusing to overwrite it")
        rows = read_legacy(root)
        ids = [row[0] for row in rows]
        if len(ids) != len(set(ids)):
            raise ValueError("Duplicate project IDs in source")
        if set(owners) != set(ids):
            raise ValueError(f"Mapping must cover exactly the source projects (missing: {sorted(set(ids) - set(owners))}; extra: {sorted(set(owners) - set(ids))})")
        users = {row[0] for row in connection.execute("SELECT username FROM users")}
        if missing := set(owners.values()) - users:
            raise ValueError(f"These users do not exist: {sorted(missing)}")
        connection.execute(SCHEMA)
        for row in rows:
            connection.execute(f"INSERT INTO projects ({COLUMNS}, username) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)", (*row, owners[row[0]]))
        connection.execute("CREATE INDEX projects_username ON projects(username)")
        connection.execute("COMMIT")
        return len(rows)
    except Exception:
        connection.execute("ROLLBACK")
        raise
    finally:
        connection.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", type=pathlib.Path, required=True)
    parser.add_argument("--projects-dir", type=pathlib.Path, required=True)
    parser.add_argument("--owners", type=pathlib.Path, required=True, help="JSON mapping project IDs to existing usernames")
    args = parser.parse_args()
    try:
        count = migrate(args.database, args.projects_dir, args.owners)
    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error) as error:
        print(f"Migration failed: {error}", file=sys.stderr)
        return 1
    print(f"Migrated {count} projects")
    return 0


if __name__ == "__main__":
    sys.exit(main())
