"""0030: observable local ranking, lazy validation, and weighted backups."""
import json
import os
import pathlib
import sqlite3
import subprocess
import tempfile
import time
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = pathlib.Path(os.environ.get('JJ_TEST_BIN', ROOT / 'target/debug/jjump')).resolve()


class LocalRankingV2(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name).resolve()
        self.home = self.root / 'home'
        self.neutral = self.home / 'neutral'
        self.neutral.mkdir(parents=True)
        self.env = {'HOME': str(self.home), 'J_JUMP_HOME': str(self.root / 'state'),
                    'PWD': str(self.neutral), 'PATH': os.environ['PATH']}
        self.cli('doctor')
        self.db = sqlite3.connect(self.root / 'state/data/visits.db')
        self.now = int(time.time())

    def tearDown(self):
        self.db.close()
        self.tmp.cleanup()

    def cli(self, *args, cwd=None, code=0):
        cwd = cwd or self.neutral
        result = subprocess.run([str(BIN), *map(str, args)], cwd=cwd,
                                env=dict(self.env, PWD=str(cwd)), capture_output=True, timeout=5)
        self.assertEqual(result.returncode, code, result.stderr)
        return result

    def directory(self, relative):
        path = self.home / relative
        path.mkdir(parents=True, exist_ok=True)
        return path

    def seed(self, rows):
        self.db.execute('DELETE FROM visits')
        self.db.executemany('INSERT INTO visits(path,key,count,last_seen,weight) VALUES(?,?,?,?,?)',
                            [(str(p), str(p).lower(), count, last, float(count)) for p, count, last in rows])
        self.db.commit()

    def chosen(self, query, expected):
        result = self.cli('--offline', 'query', query)
        self.assertEqual(result.stdout.decode().strip(), str(expected))
        self.assertEqual(result.stderr, b'')

    def test_specific_basename_beats_hot_partial_and_ancestor(self):
        exact = self.directory('one/api')
        prefix = self.directory('two/api-tools')
        internal = self.directory('three/my-api')
        ancestor = self.directory('api-project/docs')
        rows = [(exact, 1, self.now), (prefix, 100, self.now),
                (internal, 1000, self.now), (ancestor, 10000, self.now)]
        for i, expected in enumerate([exact, prefix, internal, ancestor]):
            self.seed(rows[i:])
            self.chosen('api', expected)

    def test_old_project_revisit_does_not_revive_lifetime_count(self):
        old = self.directory('old/api')
        active = self.directory('active/api')
        self.seed([(old, 1000, self.now - 365 * 86400), (active, 20, self.now)])
        self.cli('record', '--', old, cwd=old)
        self.chosen('api', active)
        count, weight = self.db.execute('SELECT count,weight FROM visits WHERE path=?', (str(old),)).fetchone()
        self.assertEqual(count, 1001)
        self.assertAlmostEqual(weight, 1.0, places=8)
        self.cli('record', '--', old, cwd=old)
        self.assertEqual(self.db.execute('SELECT count FROM visits WHERE path=?', (str(old),)).fetchone()[0], 1002)
        self.chosen('api', active)

    def test_invalid_higher_ranked_destinations_are_checked_before_return(self):
        fallback = self.directory('fallback/api-tools')
        excluded = self.directory('excluded/api')
        denied = self.directory('denied/api')
        missing = self.home / 'missing/api'
        unsafe = self.directory('unsafe/api\x1b')
        denied.chmod(0o600)
        try:
            self.cli('config', 'set', 'exclude', json.dumps([str(excluded)]))
            for bad in [excluded, denied, missing, unsafe]:
                with self.subTest(path=bad.name):
                    self.seed([(bad, 100, self.now), (fallback, 1, self.now)])
                    self.chosen('api', fallback)
            self.seed([(denied, 100, self.now)])
            self.cli('--offline', 'query', 'api', code=6)
            self.seed([(missing, 100, self.now)])
            self.cli('--offline', 'query', 'api', code=6)
            self.seed([(excluded, 100, self.now)])
            self.cli('--offline', 'query', 'api', code=3)
        finally:
            denied.chmod(0o700)

    def test_symlink_policy_is_rechecked_on_each_query(self):
        allowed = self.directory('allowed')
        blocked = self.directory('blocked')
        fallback = self.directory('api-tools')
        alias = self.home / 'alias/api'
        alias.parent.mkdir()
        alias.symlink_to(allowed, target_is_directory=True)
        self.cli('config', 'set', 'exclude', json.dumps([str(blocked)]))
        self.seed([(alias, 100, self.now), (fallback, 1, self.now)])
        self.chosen('api', alias)
        alias.unlink()
        alias.symlink_to(blocked, target_is_directory=True)
        self.chosen('api', fallback)

    def test_weighted_backup_roundtrip_retains_ranking(self):
        old = self.directory('old/api')
        active = self.directory('active/api')
        self.seed([(old, 1000, self.now), (active, 20, self.now)])
        self.db.execute('UPDATE visits SET weight=1 WHERE path=?', (str(old),))
        self.db.execute('UPDATE visits SET weight=20 WHERE path=?', (str(active),))
        self.db.commit()
        backup = self.root / 'backup/visits.json'
        self.cli('history', 'backup', backup)
        data = json.loads(backup.read_text())
        self.assertEqual(data['schema_version'], 3)
        self.assertEqual({r['weight'] for r in data['records']}, {1.0, 20.0})
        before = self.db.execute('SELECT path,count,last_seen,weight FROM visits ORDER BY path').fetchall()
        epoch = self.db.execute('SELECT epoch FROM meta').fetchone()[0]
        self.cli('history', 'clear', '--apply')
        self.cli('history', 'restore', backup, '--apply')
        self.assertEqual(self.db.execute('SELECT path,count,last_seen,weight FROM visits ORDER BY path').fetchall(), before)
        self.assertNotEqual(self.db.execute('SELECT epoch FROM meta').fetchone()[0], epoch)
        self.chosen('api', active)

    def test_invalid_backup_weight_and_legacy_backup_are_rejected_atomically(self):
        target = self.directory('one/api')
        self.seed([(target, 7, self.now)])
        backup = self.root / 'visits.json'
        record = {'id': 'd1', 'path': str(target), 'count': 7, 'last_seen': self.now}
        for weight in [None, -1, 2147483648, float('inf'), '7']:
            data = {'schema_version': 3, 'records': [dict(record, weight=weight)]}
            backup.write_text(json.dumps(data)); backup.chmod(0o600)
            before = self.db.execute('SELECT * FROM visits').fetchall()
            meta = self.db.execute('SELECT * FROM meta').fetchall()
            self.cli('history', 'restore', backup, '--apply', code=7)
            self.assertEqual(self.db.execute('SELECT * FROM visits').fetchall(), before)
            self.assertEqual(self.db.execute('SELECT * FROM meta').fetchall(), meta)
        backup.write_text(json.dumps({'schema_version': 1, 'records': [record]}))
        self.cli('history', 'restore', backup, '--apply', code=7)
        self.assertEqual(self.db.execute('SELECT count,weight FROM visits').fetchone(), (7, 7.0))
        self.chosen('api', target)

    def test_explain_and_preview_use_ranking_v2(self):
        exact = self.directory('one/api')
        hot = self.directory('api-project/docs')
        self.seed([(exact, 1, self.now), (hot, 100, self.now)])
        explanation = json.loads(self.cli('explain', '--json', 'api').stdout)
        self.assertEqual(explanation['score_version'], 2)
        self.assertEqual(explanation['lexical_matches'], 2)
        request = json.loads(self.cli('preview', 'api').stdout)
        self.assertEqual(next(v['name'] for k,v in request['questions']['destination']['criteria'].items() if k != 'none'), 'api')


if __name__ == '__main__':
    unittest.main()
