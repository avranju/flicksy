import json
import pathlib
import sqlite3
import tempfile
import unittest

from migrate_projects import migrate


class MigrationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        self.db = self.root / "flicksy.sqlite"
        self.mapping = self.root / "owners.json"
        with sqlite3.connect(self.db) as db:
            db.execute("CREATE TABLE users (username TEXT PRIMARY KEY)")
            db.executemany("INSERT INTO users VALUES (?)", [("alice",), ("bob",)])

    def test_json_import_is_atomic_and_assigns_each_owner(self):
        entries = []
        for project_id in ("one", "two"):
            folder = self.root / project_id
            folder.mkdir()
            (folder / "project.json").write_text(json.dumps({"title": project_id, "song": None, "images": [], "credits": [], "instructions": ""}))
            entries.append({"id": project_id, "path": project_id, "title": project_id})
        index = self.root / "index.json"
        index.write_text(json.dumps({"projects": entries}))
        self.mapping.write_text(json.dumps({"one": "alice"}))
        with self.assertRaises(ValueError):
            migrate(self.db, self.root, self.mapping)
        with sqlite3.connect(self.db) as db:
            self.assertEqual(db.execute("SELECT count(*) FROM sqlite_master WHERE name='projects'").fetchone()[0], 0)
        self.mapping.write_text(json.dumps({"one": "alice", "two": "bob"}))
        self.assertEqual(migrate(self.db, self.root, self.mapping), 2)
        with sqlite3.connect(self.db) as db:
            self.assertEqual(db.execute("SELECT id, username FROM projects ORDER BY id").fetchall(), [("one", "alice"), ("two", "bob")])
        self.assertTrue(index.exists())
        with self.assertRaises(ValueError):
            migrate(self.db, self.root, self.mapping)



if __name__ == "__main__":
    unittest.main()
