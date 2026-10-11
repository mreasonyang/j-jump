"""Deterministic prompt regression: keep a real previous owner suspended.

Accepted events must survive later prompts and shell exit, be serialized by
that owner, and remain bound to the history generation at submission.
"""
import contextlib
import concurrent.futures
import fcntl
import os
import pathlib
import shlex
import signal
import sqlite3
import subprocess
import tempfile
import time
import unittest

from test_0027_shell import SHELL_ARGV
from test_pty import Terminal

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = pathlib.Path(os.environ.get('JJ_TEST_BIN', ROOT / 'target/debug/jjump')).resolve()


class ObserverQueue(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name).resolve()
        self.home = self.root / 'home'
        self.home.mkdir()
        self.env = dict(os.environ, HOME=str(self.home), J_JUMP_HOME=str(self.root / 'state'),
                        PATH=str(BIN.parent) + ':' + os.environ['PATH'], NO_COLOR='1')
        for key in ('TYPESAFE_API_KEY', 'J_JUMP_CONFIG', 'J_JUMP_OFFLINE'):
            self.env.pop(key, None)
        self.alpha = self.home / 'alpha space 中文'
        self.beta = self.home / "beta ' $(touch PWNED)"
        self.seed = self.home / 'seed'
        for path in (self.alpha, self.beta, self.seed):
            path.mkdir()
        self.cli('config', 'set', 'semantic', 'off')
        self.cli('record', '--', self.seed, cwd=self.seed)
        self.db = self.root / 'state/data/visits.db'

    def tearDown(self):
        self.tmp.cleanup()

    def cli(self, *args, cwd=None, code=0):
        result = subprocess.run([str(BIN), *map(str, args)], cwd=cwd or self.home,
                                env=dict(self.env, PWD=str(cwd or self.home)),
                                capture_output=True, timeout=5)
        self.assertEqual(result.returncode, code, (args, result.stdout, result.stderr))
        return result

    def rows(self):
        with contextlib.closing(sqlite3.connect(self.db)) as db:
            return dict(db.execute('SELECT path,count FROM visits'))

    def test_generation_snapshot_waits_for_a_previous_commit(self):
        # A rollback-journal writer briefly holds EXCLUSIVE during disk sync.
        # The old 30 ms worker budget rejected this foreground snapshot before
        # the event could enter the queue, even though no worker was stuck.
        with contextlib.closing(sqlite3.connect(self.db)) as db:
            db.execute('BEGIN EXCLUSIVE')
            child = subprocess.Popen([str(BIN), 'observe', '--', str(self.beta)],
                                     cwd=self.beta, env=dict(self.env, PWD=str(self.beta)),
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                time.sleep(.2)
                self.assertIsNone(child.poll(), 'snapshot gave up before the commit finished')
            finally:
                db.rollback()
                out, err = child.communicate(timeout=2)
        self.assertEqual(child.returncode, 0, err)
        self.assertGreater(int(out), 1)
        deadline = time.monotonic() + 2
        while self.rows().get(str(self.beta)) != 1 and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertEqual(self.rows().get(str(self.beta)), 1)

    def test_generation_snapshot_still_refuses_a_stuck_store_promptly(self):
        with contextlib.closing(sqlite3.connect(self.db)) as db:
            db.execute('BEGIN EXCLUSIVE')
            started = time.monotonic()
            try:
                result = self.cli('observe', '--', self.beta, cwd=self.beta, code=7)
            finally:
                db.rollback()
        self.assertLess(time.monotonic() - started, .5)
        self.assertEqual(result.stdout, b'')
        self.assertIn(b'visit store busy', result.stderr)
        self.assertNotIn(str(self.beta), self.rows())

    def test_submission_waits_for_an_owner_critical_section(self):
        inbox = self.root / 'state/cache/observer'
        inbox.parent.mkdir(mode=0o700, exist_ok=True)
        inbox.mkdir(mode=0o700, exist_ok=True)
        lock = inbox / 'submit.lock'
        with lock.open('w') as file:
            lock.chmod(0o600)
            fcntl.flock(file, fcntl.LOCK_EX)
            child = subprocess.Popen([str(BIN), 'observe', '--', str(self.beta)],
                                     cwd=self.beta, env=dict(self.env, PWD=str(self.beta)),
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                time.sleep(.2)
                self.assertIsNone(child.poll(), 'submission gave up during owner coordination')
            finally:
                fcntl.flock(file, fcntl.LOCK_UN)
                out, err = child.communicate(timeout=2)
        self.assertEqual(child.returncode, 0, err)
        self.assertGreater(int(out), 1)
        deadline = time.monotonic() + 2
        while self.rows().get(str(self.beta)) != 1 and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertEqual(self.rows().get(str(self.beta)), 1)

    @contextlib.contextmanager
    def suspended_owner(self):
        # The DB lock ensures observe's supervisor is still alive when stopped.
        # Its worker may finish/fail while stopped; seed is intentionally not
        # part of the visit-count assertion. All target events arrive later.
        with contextlib.closing(sqlite3.connect(self.db)) as db:
            db.execute('BEGIN IMMEDIATE')
            pid = None
            try:
                for _ in range(5):
                    candidate = int(self.cli('observe', '--', self.seed, cwd=self.seed).stdout)
                    try:
                        os.kill(candidate, signal.SIGSTOP)
                        pid = candidate
                        break
                    except ProcessLookupError:
                        continue
                self.assertIsNotNone(pid, 'no live observer to suspend')
                db.rollback()
                # SIGSTOP pauses the supervisor, not an already spawned seed
                # worker. Wait for that worker to release its policy lock;
                # otherwise the later config edit legitimately reports busy.
                # A stopped owner cannot start another worker after this check.
                deadline = time.monotonic() + 2
                while time.monotonic() < deadline:
                    processes = subprocess.run(['ps', '-axo', 'pid=,ppid=,stat='],
                                               capture_output=True, text=True,
                                               check=True, timeout=2).stdout
                    rows = [line.split() for line in processes.splitlines()]
                    stopped = any(int(row[0]) == pid and row[2].startswith('T')
                                  for row in rows)
                    running_children = any(int(row[1]) == pid and not row[2].startswith('Z')
                                           for row in rows)
                    if stopped and not running_children:
                        break
                    time.sleep(.005)
                else:
                    self.fail('seed worker did not finish while its owner was stopped')
                yield pid
            finally:
                db.rollback()
                if pid is not None:
                    os.kill(pid, signal.SIGCONT)
                    deadline = time.monotonic() + 5
                    while time.monotonic() < deadline:
                        try:
                            os.kill(pid, 0)
                        except ProcessLookupError:
                            break
                        time.sleep(.01)
                    else:
                        os.kill(pid, signal.SIGKILL)
                        self.fail('observer did not drain and exit')

    def test_changed_prompts_queue_behind_live_owner_and_drain_after_shell_exit(self):
        for shell, argv in SHELL_ARGV.items():
            with self.subTest(shell=shell):
                self.cli('history', 'clear', '--apply')
                init = self.root / ('init-' + shell)
                init.write_bytes(self.cli('init', shell).stdout)
                with self.suspended_owner() as pid:
                    env = dict(self.env, PS1='JJ> ', PS2='MORE> ',
                               TERM='dumb' if shell == 'fish' else 'xterm')
                    t = Terminal(argv, env, self.alpha)
                    try:
                        assignment = ('set -g __jj_record_pid ' if shell == 'fish'
                                      else '__jj_record_pid=') + str(pid)
                        t.send(assignment + '; source ' + shlex.quote(str(init)) +
                               "; printf '\\nQUEUE-READY\\n'\n")
                        t.until(b'\r\nQUEUE-READY\r\n')
                        started = time.monotonic()
                        for i, path in enumerate((self.beta, self.alpha, self.beta)):
                            marker = 'QUEUED-' + str(i)
                            t.send('cd ' + shlex.quote(str(path)) +
                                   "; printf '\\n" + marker + "\\n'\n")
                            t.until(('\r\n' + marker + '\r\n').encode())
                        # This unchanged prompt runs only after the last changed
                        # prompt has submitted, and must not create a duplicate.
                        t.send("printf '\\nOWNER:%s\\n' \"$__jj_record_pid\"\n")
                        t.until(('\r\nOWNER:' + str(pid) + '\r\n').encode())
                        self.assertLess(time.monotonic() - started, 1)
                        self.assertNotIn(str(self.alpha), self.rows())
                        self.assertNotIn(str(self.beta), self.rows())
                    finally:
                        # There is no later shell prompt available to retry.
                        t.close()
                rows = self.rows()
                self.assertEqual(rows.get(str(self.alpha)), 2, rows)
                self.assertEqual(rows.get(str(self.beta)), 2, rows)
                self.assertFalse((self.home / 'PWNED').exists())

    def test_queued_events_cannot_restore_cleared_or_forgotten_history(self):
        for operation in ('clear', 'forget'):
            with self.subTest(operation=operation):
                with self.suspended_owner() as pid:
                    result = self.cli('observe', '--', self.beta, cwd=self.beta)
                    self.assertEqual(int(result.stdout), pid)
                    args = ['history', operation, '--apply']
                    if operation == 'forget':
                        args += ['--', self.beta]
                    self.cli(*args)
                self.assertNotIn(str(self.beta), self.rows())

    def test_queued_events_recheck_tracking_and_exclusions(self):
        for field, value in (('tracking', 'off'), ('exclude', None)):
            with self.subTest(policy=field):
                self.cli('config', 'set', 'tracking', 'on')
                self.cli('config', 'set', 'exclude', '[]')
                self.cli('history', 'clear', '--apply')
                with self.suspended_owner() as pid:
                    self.assertEqual(int(self.cli('observe', '--', self.beta, cwd=self.beta).stdout), pid)
                    if value is None:
                        import json
                        value = json.dumps([str(self.beta)])
                    self.cli('config', 'set', field, value)
                self.assertNotIn(str(self.beta), self.rows())

    def test_full_inbox_refuses_promptly_and_does_not_lose_accepted_events(self):
        accepted = 0
        with self.suspended_owner() as pid:
            for _ in range(256):
                started = time.monotonic()
                result = subprocess.run([str(BIN), 'observe', '--', str(self.beta)],
                                        cwd=self.beta, env=dict(self.env, PWD=str(self.beta)),
                                        capture_output=True, timeout=2)
                self.assertLess(time.monotonic() - started, .5)
                if result.returncode:
                    self.assertEqual(result.returncode, 7, result.stderr)
                    self.assertEqual(result.stdout, b'')
                    break
                self.assertEqual(int(result.stdout), pid)
                accepted += 1
            else:
                self.fail('suspended inbox accepted an unbounded number of events')
        self.assertGreater(accepted, 0)
        self.assertEqual(self.rows().get(str(self.beta)), accepted)

    def test_prompt_hook_preserves_failure_status(self):
        scripts = {
            'bash': 'source "$1"; false; __jj_prompt; exit $?',
            'zsh': 'source "$1"; false; __jj_prompt; exit $?',
            'fish': 'source $argv[1]; false; __jj_prompt; exit $status',
        }
        for shell, script in scripts.items():
            with self.subTest(shell=shell):
                init = self.root / ('status-' + shell)
                init.write_bytes(self.cli('init', shell).stdout)
                args = SHELL_ARGV[shell] + ['-c', script]
                args += [str(init)] if shell == 'fish' else ['test', str(init)]
                with self.suspended_owner():
                    result = subprocess.run(args, cwd=self.alpha, env=self.env,
                                            capture_output=True, timeout=3)
                    self.assertEqual(result.returncode, 1, result.stderr)

    def test_concurrent_submitters_do_not_lose_events_during_owner_retirement(self):
        def submit(_):
            return self.cli('observe', '--', self.beta, cwd=self.beta)
        owners = set()
        # Sequential batches also exercise enqueue racing the last worker's
        # reap/retirement, while several frontends submit to the same profile.
        for _ in range(8):
            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
                for result in pool.map(submit, range(4)):
                    owners.add(int(result.stdout))
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if self.rows().get(str(self.beta)) == 32:
                break
            time.sleep(.01)
        self.assertEqual(self.rows().get(str(self.beta)), 32)
        for pid in owners:
            while time.monotonic() < deadline:
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(.01)
            else:
                self.fail('concurrent observer owner did not retire')


if __name__ == '__main__':
    unittest.main()
