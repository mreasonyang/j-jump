"""Deterministic prompt regression: keep a real previous owner suspended.

Accepted events must survive later prompts and shell exit, be serialized by
that owner, and remain bound to the history epoch at submission.
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
        # A committed count is visible before the supervisor reaps its last
        # worker and finishes owner.json/socket retirement. Keep the profile
        # intact until that owner releases its lock, including on test failure.
        lock = self.root / 'state/cache/observer-v2/owner.lock'
        if lock.exists():
            with lock.open('r+') as owner:
                deadline = time.monotonic() + 5
                while True:
                    try:
                        fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        break
                    except BlockingIOError:
                        if time.monotonic() >= deadline:
                            self.fail('observer did not retire before profile cleanup')
                        time.sleep(.005)
                self.tmp.cleanup()
        else:
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

    def test_exclusive_store_lock_does_not_block_submission_or_drop_visits(self):
        # Hold longer than both former deadlines. The frontend must not read
        # SQLite, and the owned worker must retain/retry every accepted visit.
        with contextlib.closing(sqlite3.connect(self.db)) as db:
            db.execute('BEGIN EXCLUSIVE')
            started = time.monotonic()
            owners = {int(self.cli('observe', '--', self.beta, cwd=self.beta).stdout)
                      for _ in range(4)}
            self.assertLess(time.monotonic() - started, .2)
            self.assertEqual(len(owners), 1)
            time.sleep(.7)
            os.kill(next(iter(owners)), 0)
            db.rollback()
        deadline = time.monotonic() + 3
        while self.rows().get(str(self.beta)) != 4 and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertEqual(self.rows().get(str(self.beta)), 4)

    def test_all_shells_queue_during_exclusive_store_lock_and_drain_after_exit(self):
        for shell, argv in SHELL_ARGV.items():
            with self.subTest(shell=shell):
                self.cli('history', 'clear', '--apply')
                init = self.root / ('locked-' + shell)
                init.write_bytes(self.cli('init', shell).stdout)
                with contextlib.closing(sqlite3.connect(self.db)) as db:
                    db.execute('BEGIN EXCLUSIVE')
                    t = Terminal(argv, dict(self.env, PS1='JJ> ', PS2='MORE> ',
                                           TERM='dumb' if shell == 'fish' else 'xterm'),
                                 self.alpha)
                    try:
                        t.send('source ' + shlex.quote(str(init)) +
                               "; printf '\\nLOCK-READY\\n'\n")
                        t.until(b'\r\nLOCK-READY\r\n')
                        for i, path in enumerate((self.beta, self.alpha, self.beta)):
                            marker = 'LOCKED-' + str(i)
                            t.send('cd ' + shlex.quote(str(path)) +
                                   "; printf '\\n" + marker + "\\n'\n")
                            t.until(('\r\n' + marker + '\r\n').encode())
                        t.send("printf '\\nLOCK-PID:%s\\n' \"$__jj_record_pid\"\n")
                        out = t.until(b'\r\nLOCK-PID:')
                        self.assertNotIn(b'visit store busy', out)
                    finally:
                        t.close()
                    time.sleep(.7)
                    db.rollback()
                deadline = time.monotonic() + 3
                while time.monotonic() < deadline:
                    rows = self.rows()
                    if rows.get(str(self.alpha)) == rows.get(str(self.beta)) == 2:
                        break
                    time.sleep(.01)
                self.assertEqual(rows.get(str(self.alpha)), 2, rows)
                self.assertEqual(rows.get(str(self.beta)), 2, rows)

    def test_stopped_worker_is_retained_and_serializes_later_visits(self):
        with contextlib.closing(sqlite3.connect(self.db)) as db:
            db.execute('BEGIN EXCLUSIVE')
            pid = int(self.cli('observe', '--', self.beta, cwd=self.beta).stdout)
            child = None
            try:
                deadline = time.monotonic() + 2
                while time.monotonic() < deadline:
                    rows = subprocess.check_output(['ps', '-axo', 'pid=,ppid=,stat='],
                                                   text=True).splitlines()
                    children = [int(row.split()[0]) for row in rows
                                if int(row.split()[1]) == pid and
                                not row.split()[2].startswith('Z')]
                    self.assertLessEqual(len(children), 1)
                    if children:
                        child = children[0]
                        os.kill(child, signal.SIGSTOP)
                        break
                    time.sleep(.005)
                self.assertIsNotNone(child)
                for _ in range(3):
                    self.assertEqual(int(self.cli('observe', '--', self.beta,
                                                 cwd=self.beta).stdout), pid)
                time.sleep(.4)
                os.kill(child, 0)  # The former 250ms kill would discard this event.
            finally:
                db.rollback()
                if child is not None:
                    os.kill(child, signal.SIGCONT)
        deadline = time.monotonic() + 3
        while self.rows().get(str(self.beta)) != 4 and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertEqual(self.rows().get(str(self.beta)), 4)

    def test_current_queue_is_independent_of_previous_protocol_locks(self):
        # An old supervisor may still own its protocol while a new executable
        # is installed. Never acknowledge a new event to that old inbox.
        import json
        old = self.root / 'state/cache/observer'
        old.parent.mkdir(mode=0o700)
        old.mkdir(mode=0o700)
        state = old / 'owner.json'
        state.write_text(json.dumps({'pid': os.getpid(), 'outstanding': 1}))
        state.chmod(0o600)
        original = state.read_bytes()
        with (old / 'submit.lock').open('w') as submit, (old / 'owner.lock').open('w') as owner:
            for file in (submit, owner):
                os.fchmod(file.fileno(), 0o600)
                fcntl.flock(file, fcntl.LOCK_EX)
            result = self.cli('observe', '--', self.beta, cwd=self.beta)
            self.assertNotEqual(int(result.stdout), os.getpid())
            deadline = time.monotonic() + 2
            while self.rows().get(str(self.beta)) != 1 and time.monotonic() < deadline:
                time.sleep(.01)
            self.assertEqual(self.rows().get(str(self.beta)), 1)
            self.assertEqual(state.read_bytes(), original)

    def test_submission_waits_for_an_owner_critical_section(self):
        inbox = self.root / 'state/cache/observer-v2'
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
        for operation in ('clear', 'forget', 'restore'):
            with self.subTest(operation=operation):
                backup = self.root / 'backup.json'
                if operation == 'restore':
                    self.cli('history', 'backup', backup)
                with self.suspended_owner() as pid:
                    result = self.cli('observe', '--', self.beta, cwd=self.beta)
                    self.assertEqual(int(result.stdout), pid)
                    args = ['history', operation, '--apply']
                    if operation == 'forget':
                        args += ['--', self.beta]
                    elif operation == 'restore':
                        args += [backup]
                    self.cli(*args)
                self.assertNotIn(str(self.beta), self.rows())

    def test_replaced_store_invalidates_older_queued_visits(self):
        with self.suspended_owner() as pid:
            self.assertEqual(int(self.cli('observe', '--', self.beta, cwd=self.beta).stdout), pid)
            self.db.rename(self.db.with_suffix('.old'))
            self.cli('record', '--', self.seed, cwd=self.seed)
        self.assertNotIn(str(self.beta), self.rows())

    def test_history_fence_files_refuse_shared_permissions_and_links(self):
        self.cli('history', 'clear', '--apply')
        for name in ('observation.lock', 'observation.epoch'):
            path = self.db.parent / name
            with self.subTest(file=name, problem='permissions'):
                path.chmod(0o644)
                try:
                    self.assertEqual(self.cli('observe', '--', self.beta,
                                              cwd=self.beta, code=7).stdout, b'')
                finally:
                    path.chmod(0o600)
            with self.subTest(file=name, problem='symlink'):
                held = path.with_suffix('.held')
                path.rename(held)
                path.symlink_to(held)
                try:
                    self.assertEqual(self.cli('observe', '--', self.beta,
                                              cwd=self.beta, code=7).stdout, b'')
                finally:
                    path.unlink()
                    held.rename(path)
            with self.subTest(file=name, problem='hardlink'):
                alias = path.with_suffix('.alias')
                os.link(path, alias)
                try:
                    self.assertEqual(self.cli('observe', '--', self.beta,
                                              cwd=self.beta, code=7).stdout, b'')
                finally:
                    alias.unlink()
        self.assertNotIn(str(self.beta), self.rows())

    def test_pruning_invalidates_older_queued_visits(self):
        gone = self.home / 'gone'
        gone.mkdir()
        self.cli('record', '--', gone, cwd=gone)
        gone.rmdir()
        with self.suspended_owner() as pid:
            self.assertEqual(int(self.cli('observe', '--', self.beta, cwd=self.beta).stdout), pid)
            self.cli('history', 'prune', '--apply')
        self.assertNotIn(str(self.beta), self.rows())
        self.assertNotIn(str(gone), self.rows())

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
