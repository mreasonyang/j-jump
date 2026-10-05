"""PTY wizard checks; only fixture binary saves keys, to a memory-only backend."""
import contextlib
import json
import os
from pathlib import Path
import shlex
import termios
import unittest

import test_0036_first_use as first
from test_pty import Terminal

ROOT = Path(__file__).resolve().parents[2]
BIN = first.BIN
FIXTURE = ROOT / 'target/debug/examples/setup-credentials-fixture'
SECRET = 'synthetic-setup-value-中文'

class SetupKeys(unittest.TestCase):
    setUp = first.FirstUse.setUp
    profile = first.FirstUse.profile
    cli = first.FirstUse.cli
    shell = first.FirstUse.shell

    @contextlib.contextmanager
    def process(self, fixture=False, extra=None):
        if fixture:
            self.assertTrue(FIXTURE.is_file(), 'build the setup-credentials-fixture example first')
        t = Terminal([str(FIXTURE if fixture else BIN), 'setup'], dict(self.env, **(extra or {})), self.cwd)
        try:
            yield t
        finally:
            self.assertFalse(SECRET.encode() in t.output, 'secret appeared in terminal output')
            t.close()

    def jev(self, t):
        t.until(b'Semantic provider'); t.send('jev\r')
        t.until(b'Network permission'); t.send('\r')
        t.until(b'Fields'); t.send('\r')
        t.until(b'API key [')

    def enter(self, t, secret=SECRET):
        t.send('enter\r'); t.until(b'Jev API key (hidden;')
        self.assertFalse(termios.tcgetattr(t.fd)[3] & termios.ECHO)
        t.send(secret + '\r')

    def final_prompt(self, t):
        t.until(b'Local visit tracking'); t.send('off\r')
        t.until(b'Advanced settings')
        self.assertFalse(self.config.exists())
        self.assertFalse((self.config.parent / 'credential.epoch').exists())

    def test_hidden_draft_cancel_and_terminal_restore(self):
        with self.process() as t:
            self.jev(t); self.enter(t); self.final_prompt(t)
            self.assertTrue(termios.tcgetattr(t.fd)[3] & termios.ECHO)
            t.send('q\r'); t.cancelled()
            self.assertFalse(self.config.exists())
            self.assertNotIn(b'jjump credential set', t.output)

    def test_ctrl_c_eof_and_empty_input(self):
        for control in ('\x03', '\x04'):
            with self.subTest(control=repr(control)), self.process() as t:
                # 0046: an empty hidden key keeps "no key" and setup proceeds; go back to retry.
                self.jev(t); self.enter(t, '')
                t.until(b'Local visit tracking'); self.assertIn(b'No key entered', t.output); t.send('b\r')
                t.until(b'Semantic provider'); t.send('\r')
                t.until(b'Network permission'); t.send('\r')
                t.until(b'Fields'); t.send('\r')
                t.until(b'API key [')
                t.send('enter\r'); t.until(b'Jev API key (hidden;')
                t.send(SECRET + control); t.cancelled()
                self.assertTrue(termios.tcgetattr(t.fd)[3] & termios.ECHO)
                self.assertFalse(self.config.exists())

    def test_invalid_and_oversized_input_can_retry(self):
        with self.process() as t:
            self.jev(t)
            for secret in ('x' * 4097, 'control\tvalue'):
                self.enter(t, secret)
                t.until(b'API key [')
                self.assertNotIn(b'Local visit tracking', t.output)
            self.enter(t); self.final_prompt(t)
            t.send('q\r'); t.cancelled()

    def test_reset_discards_key_without_store_access(self):
        with self.process() as t:
            self.jev(t); self.enter(t); self.final_prompt(t)
            t.send('reset\r'); t.until(b'Saved.')
        cfg = json.loads(self.config.read_text())
        self.assertFalse(cfg['semantic']); self.assertEqual(cfg['credential'], 'environment')

    def test_existing_menu_keep_and_environment(self):
        self.env['TYPESAFE_API_KEY'] = 'synthetic-environment-value'
        self.cli('config', 'set', 'semantic', 'on')
        with self.process() as t:
            t.until(b's Save; q Exit without saving'); t.send('6\r')
            t.until(b'API key ['); self.assertIn(b'overrides any stored key', t.output)
            t.send('environment\r'); t.until(b's Save; q Exit without saving')
            t.send('s\r'); t.until(b'Saved.')
        self.assertEqual(json.loads(self.config.read_text())['credential'], 'environment')

    def test_fixture_save_and_retry_without_os_store(self):
        for fail in (False, True):
            self.profile('fixture' + str(fail))
            with self.subTest(fail=fail), self.process(True, {'FIXTURE_FAIL_ONCE': '1'} if fail else {}) as t:
                self.jev(t); self.enter(t); self.final_prompt(t)
                t.send('\r')
                if fail:
                    t.until(b's Save; q Exit without saving')
                    self.assertFalse(self.config.exists())
                    t.send('s\r')
                t.until(b'Saved.')
                self.assertIn(b'FIXTURE: credential stored in memory', t.output)
            self.assertEqual(json.loads(self.config.read_text())['credential'], 'system')
            self.assertFalse(SECRET in self.config.read_text())
            # Keeping an existing configured source must not access the real store.
            with self.process() as t:
                t.until(b's Save; q Exit without saving'); t.send('6\r')
                t.until(b'API key ['); t.send('keep\r')
                t.until(b's Save; q Exit without saving'); t.send('q\r'); t.cancelled()

    def test_inline_j_ji_reaches_key_entry_in_three_shells(self):
        for shell in first.SHELLS:
            for command in ('j', 'ji'):
                self.profile(shell + command)
                with self.subTest(shell=shell, command=command), self.shell(shell) as t:
                    t.send(command + ' -- ' + shlex.quote(str(self.target)) + '\r')
                    self.jev(t)
                    self.enter(t, SECRET + '\x03')
                    t.cancelled()
                    self.assertFalse(SECRET.encode() in t.output)
                    self.assertFalse(self.config.exists())
                    t.cmd('printf "WHERE:%s\\n" "$PWD"', 'KEYCANCEL')
                    self.assertIn(('WHERE:' + str(self.cwd)).encode(), t.output)

    def test_back_at_first_step_stays_and_key_back_returns_to_network(self):
        with self.process() as t:
            t.until(b'Semantic provider'); t.send('b\r')
            self.jev(t)
            t.send('b\r'); t.until(b'Semantic provider'); t.send('off\r')
            self.final_prompt(t); t.send('q\r'); t.cancelled()

    def test_disabling_semantic_discards_pending_source(self):
        with self.process() as t:
            self.jev(t); self.enter(t); self.final_prompt(t)
            t.send('b\r'); t.until(b'Local visit tracking'); t.send('b\r')
            t.until(b'Semantic provider'); t.send('off\r')
            self.final_prompt(t); t.send('\r'); t.until(b'Saved.')
        cfg = json.loads(self.config.read_text())
        self.assertFalse(cfg['semantic']); self.assertEqual(cfg['credential'], 'environment')

    def test_config_conflict_rejected_before_keychain_access(self):
        with self.process() as t:
            self.jev(t); self.enter(t); self.final_prompt(t)
            self.cli('config', 'set', 'tracking', 'off')
            before = self.config.read_bytes()
            t.send('\r'); t.until(b'J7')
            self.assertEqual(before, self.config.read_bytes())
            self.assertFalse((self.config.parent / 'credential.epoch').exists())
