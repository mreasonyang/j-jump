"""Current-only development: explicit outcomes, no legacy adoption or implicit routes.

All provider replies below are local Unix-socket fixtures. No real key/network.
"""
import contextlib
import hashlib
import json
import os
import pathlib
import shlex
import shutil
import socket
import sqlite3
import struct
import threading
import time
import unittest
import test_0032_ux as ux


class CurrentContract(unittest.TestCase):
    setUp = ux.UXTasks.setUp
    tearDown = ux.UXTasks.tearDown
    cli = ux.UXTasks.cli
    config = ux.UXTasks.config
    seed = ux.UXTasks.seed
    shell = ux.UXTasks.shell
    process = ux.UXTasks.process

    def test_config_requires_current_complete_shape_without_conversion(self):
        self.cli('config', 'set', 'tracking', 'off')
        config = self.config()
        current = json.loads(config.read_text())
        self.assertEqual(current['schema_version'], 3)
        invalid = [{}, dict(current, schema_version=1), dict(current, schema_version=2), dict(current, request_limit=10), dict(current, schema_version=99),
                   dict(current, proxy='none'), dict(current, legacy=True)]
        invalid.extend({k: v for k, v in current.items() if k != field} for field in current)
        for value in invalid:
            with self.subTest(keys=list(value)):
                config.write_text(json.dumps(value))
                before = config.read_bytes()
                self.cli('config', 'show', '--json', code=7)
                self.cli('config', 'set', 'tracking', 'on', code=7)
                self.assertEqual(config.read_bytes(), before)
        config.write_text(json.dumps(current))
        self.cli('config', 'show', '--json')

    def test_backup_accepts_only_current_schema_and_mandatory_weight(self):
        self.seed()
        backup = self.root / 'records.json'
        self.cli('history', 'backup', backup)
        current = json.loads(backup.read_text())
        self.assertEqual(current['schema_version'], 3)
        self.assertIsInstance(current['records'][0]['weight'], (float, int))
        variants = [dict(current, schema_version=v) for v in (0, 1, 2, 4)]
        missing = json.loads(json.dumps(current)); del missing['records'][0]['weight']
        null = json.loads(json.dumps(current)); null['records'][0]['weight'] = None
        variants += [missing, null]
        store = self.state / 'data/visits.db'
        for value in variants:
            with self.subTest(value=value):
                backup.write_text(json.dumps(value))
                before = store.read_bytes()
                self.cli('history', 'restore', backup, '--apply', code=7)
                self.assertEqual(store.read_bytes(), before)
        backup.write_text(json.dumps(current))
        self.cli('history', 'clear', '--apply')
        self.cli('history', 'restore', backup, '--apply')
        self.assertEqual(self.cli('--offline', 'query', 'alpha').stdout.strip(), os.fsencode(self.target))

    def test_fresh_store_is_complete_and_does_not_repair_null_weight(self):
        self.seed()
        with sqlite3.connect(self.state / 'data/visits.db') as db:
            self.assertEqual(db.execute('PRAGMA user_version').fetchone()[0], 4)
            weight = next(row for row in db.execute('PRAGMA table_info(visits)') if row[1] == 'weight')
            self.assertEqual(weight[3], 1)
            with self.assertRaises(sqlite3.IntegrityError):
                db.execute('UPDATE visits SET weight=NULL')
            self.assertEqual(db.execute('SELECT weight FROM visits').fetchone()[0], 1.0)

    def test_default_picker_does_not_auto_run_fzf_and_unknown_mode_fails(self):
        self.seed(); self.cli('config', 'set', 'tracking', 'off')
        fake = self.root / 'fake'; fake.mkdir()
        marker = self.root / 'CALLED'
        helper = fake / 'fzf'
        helper.write_text('#!/bin/sh\ntouch ' + shlex.quote(str(marker)) + '\nexit 1\n'); helper.chmod(0o700)
        self.env['PATH'] = str(fake) + ':' + self.env['PATH']
        self.env.pop('J_JUMP_PICKER')
        for shell in ('bash', 'zsh', 'fish'):
            with self.subTest(shell=shell), self.shell(shell) as t:
                t.send('ji\r'); t.until(b'Empty/q: cancel'); t.send('q\r'); t.cancelled()
                self.assertFalse(marker.exists())
        self.env['J_JUMP_PICKER'] = 'typo'
        with self.process('query', '--interactive') as t:
            t.until(b'J2:'); self.assertNotIn(b'Choose a directory', t.output)

    def test_no_match_does_not_broaden_but_bare_local_browse_works(self):
        self.seed(); self.cli('config', 'set', 'tracking', 'off')
        for shell in ('bash', 'zsh', 'fish'):
            with self.subTest(shell=shell), self.shell(shell) as t:
                t.send('ji not-in-history\r'); t.until(b'J3:')
                self.assertNotIn(b'Choose a directory', t.output)
                t.send('ji\r'); t.until(b'Choose a directory'); t.send('1\r'); t.drain(.15)
                t.cmd('printf "WHERE:%s\\n" "$PWD"', 'BROWSED')
                self.assertIn(os.fsencode('WHERE:' + str(self.target)), t.output)

    def configure_semantics(self, suffix):
        self.state = self.root / ('state-' + suffix)
        self.env['J_JUMP_HOME'] = str(self.state)
        self.env['TYPESAFE_API_KEY'] = 'synthetic-current-contract-key'
        self.seed()
        for key, value in [('semantic', 'on'), ('consent', 'always'), ('tracking', 'off')]:
            self.cli('config', 'set', key, value)

    def runtime(self):
        keys = ('HTTPS_PROXY', 'https_proxy', 'HTTP_PROXY', 'http_proxy', 'ALL_PROXY', 'all_proxy', 'NO_PROXY', 'no_proxy', 'REQUEST_METHOD')
        profile = os.fsencode(self.config()) + b'\0' + os.fsencode(self.state / 'cache') + b'\0adapter-v4'
        profile += b''.join(k.encode() + b'\0' + self.env.get(k, '').encode() + b'\0' for k in keys)
        return pathlib.Path('/private/tmp' if os.uname().sysname == 'Darwin' else '/tmp') / ('jj-' + str(os.geteuid()) + '-' + hashlib.sha256(profile).hexdigest()[:24])

    @staticmethod
    def recv_exact(conn, size):
        data = b''
        while len(data) < size:
            part = conn.recv(size-len(data))
            if not part: raise EOFError('fixture client closed')
            data += part
        return data

    @contextlib.contextmanager
    def adapter_reply(self, outcome):
        runtime = self.runtime(); runtime.mkdir(mode=0o700)
        listener = socket.socket(socket.AF_UNIX); listener.bind(str(runtime / 'jev.sock'))
        (runtime / 'jev.sock').chmod(0o600); listener.listen(); listener.settimeout(.1)
        stop = threading.Event(); frames = []; errors = []
        def serve():
            while not stop.is_set():
                try: conn, _ = listener.accept()
                except socket.timeout: continue
                except OSError: return
                with conn:
                    try:
                        conn.settimeout(2)
                        size = struct.unpack('>I', self.recv_exact(conn, 4))[0]
                        frame = json.loads(self.recv_exact(conn, size)); frames.append(frame['kind'])
                        if outcome == 'deadline':
                            stop.wait(12); continue
                        req = json.loads(frame['payload']); ids = list(req['questions']['destination']['criteria'])
                        choice = 'none' if outcome == 'none' else next(k for k in ids if k != 'none')
                        answer = {'type': 'choice', 'choice': choice, 'confidence': 1.0,
                                  'probabilities': {k: float(k == choice) for k in ids}}
                        answers = {k: {'type': 'noul', 'noul': 1.0} for k in req['questions'] if k != 'destination'}
                        answers['destination'] = answer
                        body = json.dumps({'model': req['model'], 'answers': answers})
                        reply = {'version': 4, 'id': frame['id'], 'status': 'auth' if outcome == 'error' else 'ok',
                                 'body': 'not-json' if outcome == 'invalid' else body}
                        if outcome == 'binding': reply['id'] = 'bad-binding'
                        data = json.dumps(reply).encode(); conn.sendall(struct.pack('>I', len(data)) + data)
                    except (BrokenPipeError, ConnectionResetError): pass
                    except Exception as exc: errors.append(type(exc).__name__)
        worker = threading.Thread(target=serve); worker.start()
        try: yield frames
        finally:
            stop.set(); listener.close(); worker.join(3)
            self.assertFalse(worker.is_alive()); self.assertEqual(errors, [])
            shutil.rmtree(runtime)

    def assert_dispatch(self):
        with sqlite3.connect(self.state / 'cache/semantic-cache.db') as db:
            self.assertLessEqual(db.execute('SELECT count(*) FROM dispatch').fetchone()[0], 1)

    def test_provider_failures_never_switch_route_in_three_shells(self):
        commands = {'j': 'j notlexical\r', 'ji': 'ji notlexical\r',
                    'forced': 'j --force-semantic alpha\r', 'completion': 'j notlexical \t'}
        for shell in ('bash', 'zsh', 'fish'):
            for outcome in ('error', 'none', 'invalid', 'binding'):
                for route, command in commands.items():
                    with self.subTest(shell=shell, outcome=outcome, route=route):
                        self.configure_semantics(shell + outcome + route)
                        with self.shell(shell) as t, self.adapter_reply(outcome) as frames:
                            t.send(command); t.until(b'J3:' if outcome == 'none' else b'J5:')
                            self.assertNotIn(b'Choose a directory', t.output)
                            if route == 'completion':
                                self.assertNotIn(b'j -- ', t.output); t.send('\x15')
                            t.cmd('printf "WHERE:%s\\n" "$PWD"', 'STOPPED')
                            self.assertIn(os.fsencode('WHERE:' + str(self.cwd)), t.output)
                            self.assertNotIn(self.env['TYPESAFE_API_KEY'].encode(), t.output)
                            self.assertEqual(frames, ['Send']); self.assert_dispatch()

    def test_valid_provider_suggestion_requires_selection_in_three_shells(self):
        for shell in ('bash', 'zsh', 'fish'):
            self.configure_semantics('valid-' + shell)
            with self.subTest(shell=shell), self.shell(shell) as t, self.adapter_reply('valid') as frames:
                t.send('ji notlexical\r'); t.until(b'Empty/q: cancel')
                self.assertIn(b'[Jev]', t.output)
                t.send('1\r'); t.drain(.15)
                t.cmd('printf "WHERE:%s\\n" "$PWD"', 'SELECTED')
                self.assertIn(os.fsencode('WHERE:' + str(self.target)), t.output)
                self.assertEqual(frames, ['Send']); self.assert_dispatch()

    def test_deadline_does_not_open_picker_in_three_shells(self):
        for shell in ('bash', 'zsh', 'fish'):
            self.configure_semantics('deadline-' + shell)
            with self.subTest(shell=shell), self.shell(shell) as t, self.adapter_reply('deadline') as frames:
                started = time.monotonic(); t.send('ji notlexical\r')
                t.until(b'[Enter=yes / W=wait]'); t.send('W'); t.until(b'J5:', seconds=13)
                self.assertLess(time.monotonic()-started, 12)
                self.assertNotIn(b'Choose a directory', t.output)
                t.cmd('printf "WHERE:%s\\n" "$PWD"', 'TIMEOUT')
                self.assertIn(os.fsencode('WHERE:' + str(self.cwd)), t.output)
                self.assertEqual(frames, ['Send']); self.assert_dispatch()

    def test_unavailable_adapter_never_dispatches_direct_http(self):
        # A deliberately stopped private socket fails before request dispatch.
        proxy = socket.socket(); proxy.bind(('127.0.0.1', 0)); proxy.listen(); proxy.settimeout(.2)
        self.env['HTTPS_PROXY'] = 'http://127.0.0.1:' + str(proxy.getsockname()[1])
        self.configure_semantics('unavailable')
        runtime = self.runtime(); runtime.mkdir(mode=0o700)
        (runtime / 'jev.sock').write_text('not a socket'); (runtime / 'jev.sock').chmod(0o600)
        try:
            with self.process('--force-semantic', 'query', 'alpha') as t:
                t.until(b'J7:'); self.assertNotIn(b'Choose a directory', t.output)
            with self.assertRaises(socket.timeout): proxy.accept()
            with sqlite3.connect(self.state / 'cache/semantic-cache.db') as db:
                self.assertEqual(db.execute('SELECT count(*) FROM dispatch').fetchone()[0], 0)
        finally:
            proxy.close(); shutil.rmtree(runtime)
