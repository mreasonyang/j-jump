"""Real local HTTP/IPC and shell interaction, with synthetic model responses."""
import contextlib
import http.server
import json
import os
from pathlib import Path
import shlex
import sqlite3
import subprocess
import threading
import time
import unittest

from test_navigation_fixes import Terminal, BIN
import test_0036_first_use as first
from test_pty import Terminal as Process


class Tev1(unittest.TestCase):
    setUp = first.FirstUse.setUp
    profile = first.FirstUse.profile

    def cli(self, *args, code=0, cwd=None):
        r = subprocess.run([str(BIN), *map(str, args)], env=dict(self.env, PWD=str(cwd or self.cwd)),
                           cwd=cwd or self.cwd, capture_output=True, timeout=14)
        self.assertEqual(r.returncode, code, r.stderr)
        return r

    @contextlib.contextmanager
    def server(self, outcome='ok'):
        requests = []
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_): pass
            def reply(self, body, status=200):
                raw = json.dumps(body).encode()
                self.send_response(status); self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(raw))); self.end_headers()
                try: self.wfile.write(raw)
                except (BrokenPipeError, ConnectionResetError): pass
            def do_GET(self):
                requests.append((self.path, dict(self.headers), None))
                if self.path == '/api/version': self.reply({'version': '0.34.0' if outcome == 'old' else '0.35.1'})
                else: self.reply({'models': [] if outcome == 'missing' else [{'name': 'tev1:4b', 'digest': 'synthetic-digest', 'details': {'format': 'mlx' if outcome == 'mlx' else 'gguf'}}]})
            def do_POST(self):
                data = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                requests.append((self.path, dict(self.headers), data))
                if outcome == 'slow': time.sleep(3)
                if outcome == 'timeout': time.sleep(11)
                if outcome == 'redirect':
                    self.send_response(302); self.send_header('Location', 'http://example.invalid/'); self.end_headers(); return
                criteria = data['questions']['destination']['criteria']
                choice = next((k for k, v in criteria.items() if k == 'ready' or 'docs' in str(v)), 'none')
                if outcome == 'none': choice = 'none'
                if outcome == 'unknown': choice = 'forged'
                self.reply({'model': data['model'], 'answers': {'destination': {
                    'type': 'choice', 'choice': choice, 'probabilities': {k: float(k == choice) for k in criteria}, 'confidence': 1.0}},
                    'usage': {'input_tokens': 300, 'output_tokens': 1}})
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
        try: yield f'http://127.0.0.1:{server.server_port}', requests
        finally:
            server.shutdown(); server.server_close(); thread.join(3)

    def configure(self, url):
        self.env.pop('J_JUMP_OFFLINE', None)
        # Bad credentials and proxies must have no influence on a local driver.
        self.env.update(TYPESAFE_API_KEY='invalid\nsynthetic', CLOUDFLARE_AUTH_TOKEN='invalid\nsynthetic',
                        HTTP_PROXY='http://127.0.0.1:1', HTTPS_PROXY='http://127.0.0.1:1', ALL_PROXY='http://127.0.0.1:1')
        self.cli('config', 'set', 'provider', 'tev1')
        self.cli('config', 'set', 'ollama_url', url)
        self.cli('config', 'set', 'semantic', 'on')
        self.cli('config', 'set', 'consent', 'always')
        self.docs = self.home / 'docs'; self.docs.mkdir(exist_ok=True)
        self.cli('record', '--', self.docs, cwd=self.docs)
        self.cli('record', '--', self.target, cwd=self.target)
        self.cli('config', 'set', 'tracking', 'off')

    @contextlib.contextmanager
    def shell(self, name):
        init = self.root / (name + '.init'); init.write_bytes(self.cli('init', name).stdout)
        t = Terminal(name, self.env, self.cwd)
        try:
            t.cmd('source ' + shlex.quote(str(init)), 'READY')
            yield t
        finally: t.close(); self.cli('adapter', 'stop')

    def test_diagnostics_are_explicit_and_credentials_are_unused(self):
        with self.server() as (url, requests):
            self.configure(url)
            for args in [('doctor', '--json'), ('config', 'show', '--json'), ('preview', '文档'), ('credential', 'status')]: self.cli(*args)
            self.assertEqual(requests, [])
            report = json.loads(self.cli('provider-check', '--json').stdout)
            self.assertEqual(report['synthetic_request'], 'passed')
            self.assertEqual(len(requests), 3)
            self.assertTrue(all('Authorization' not in headers for _, headers, _ in requests))
            self.cli('--offline', 'provider-check', code=2)
            self.cli('credential', 'delete', '--apply', code=2)
            self.assertFalse((self.state / 'config/credential.epoch').exists())

    def test_diagnostic_refuses_unsupported_runtime_missing_model_and_mlx(self):
        for outcome, expected in [('old', b'version is unsupported'), ('missing', b'model is missing'), ('mlx', b'GGUF')]:
            with self.subTest(outcome=outcome), self.server(outcome) as (url, requests):
                self.profile(outcome); self.configure(url)
                self.assertIn(expected, self.cli('provider-check', code=5).stderr)
                self.assertFalse(any(data is not None for _, _, data in requests))

    def test_select_and_cancel_in_each_shell_over_real_http(self):
        for name in ('bash', 'zsh', 'fish'):
            with self.subTest(shell=name), self.server() as (url, requests):
                self.profile(name); self.configure(url)
                with self.shell(name) as t:
                    t.send('ji 文档\r'); t.until(b'Empty/q: cancel', seconds=8); t.send('1\r'); t.drain(.3)
                    t.cmd('printf "WHERE:%s\\n" "$PWD"', 'SELECTED')
                    self.assertIn(('WHERE:' + str(self.docs)).encode(), t.output)
                    t.cmd('cd ' + shlex.quote(str(self.cwd)), 'RETURNED')
                    start = len(t.output)
                    t.send('ji 文档\r'); t.until(b'Empty/q: cancel', seconds=8); t.send('q\r'); t.drain(.3)
                    t.cmd('printf "STILL:%s\\n" "$PWD"', 'CANCELLED')
                    self.assertIn(('STILL:' + str(self.cwd)).encode(), t.output[start:])
                    self.assertEqual(len(requests), 2)  # no stale local model cache
                    for path, headers, data in requests:
                        self.assertEqual(path, '/v1/systemone'); self.assertNotIn('Authorization', headers)
                        self.assertTrue(all(isinstance(v, str) for v in data['questions']['destination']['criteria'].values()))
                with sqlite3.connect(self.state / 'cache/semantic-drivers.db') as db:
                    self.assertEqual(db.execute('SELECT count(*) FROM cache').fetchone()[0], 0)

    def test_abstention_invalid_reply_and_redirect_never_open_picker(self):
        for outcome in ('none', 'unknown', 'redirect'):
            with self.subTest(outcome=outcome), self.server(outcome) as (url, requests):
                self.profile(outcome); self.configure(url)
                with self.shell('zsh') as t:
                    t.send('ji 文档\r'); t.until(b'J3:' if outcome == 'none' else b'J5:', seconds=8)
                    self.assertNotIn(b'Empty/q: cancel', t.output)
                    t.cmd('printf "STILL:%s\\n" "$PWD"', 'DONE')
                    self.assertIn(('STILL:' + str(self.cwd)).encode(), t.output)
                self.assertEqual(len(requests), 1)

    def test_wait_cancellation_leaves_directory_and_releases_runtime(self):
        with self.server('slow') as (url, requests):
            self.configure(url)
            with self.shell('zsh') as t:
                t.send('ji 文档\r'); t.until(b'[Enter=yes / W=wait]', seconds=5)
                start=time.monotonic(); t.send('\x03'); t.drain(.3)
                t.cmd('printf "STILL:%s\\n" "$PWD"', 'CANCELLED')
                self.assertLess(time.monotonic()-start, 2)
                self.assertIn(('STILL:' + str(self.cwd)).encode(), t.output)
                self.assertNotIn(b'Empty/q: cancel', t.output)
                self.assertEqual(len(requests), 1)

    def test_first_setup_has_local_fields_and_no_key_prompt(self):
        self.env.pop('J_JUMP_OFFLINE', None)
        t = Process([str(BIN), 'setup'], self.env, self.cwd)
        try:
            t.until(b'Semantic provider'); t.send('tev1\r')
            t.until(b'Request permission'); t.send('\r')
            t.until(b'Fields'); t.send('\r')
            t.until(b'Ollama URL'); t.send('\r')
            t.until(b'Local visit tracking'); t.send('\r')
            t.until(b'Advanced settings'); t.send('\r'); t.until(b'Saved.')
            self.assertNotIn(b'API key [', t.output)
        finally: t.close()
        self.assertEqual(json.loads(self.config.read_text())['provider'], 'tev1')
        self.assertIn(b'not checked', self.cli('doctor').stdout)
    def test_chinese_local_readiness_names_service_without_key_requirement(self):
        with self.server() as (url, requests):
            self.configure(url); self.env['J_JUMP_LANG']='zh'
            output=self.cli('doctor').stdout.decode()
            self.assertIn('Tev1 4B',output); self.assertIn('无需凭据',output); self.assertIn('尚未检查',output)
            self.assertEqual(requests,[])



if __name__ == '__main__': unittest.main()
