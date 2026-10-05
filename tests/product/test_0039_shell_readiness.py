"""Real PTY startup regression; no product changes or external service calls."""
import os
from pathlib import Path
import shlex
import shutil
import tempfile
import time
import unittest
from test_navigation_fixes import Terminal


class ShellReadiness(unittest.TestCase):
    def test_all_shells_reach_prompt_before_input(self):
        with tempfile.TemporaryDirectory() as directory:
            for shell in ('bash', 'zsh', 'fish'):
                with self.subTest(shell=shell):
                    env = {'HOME': directory, 'PATH': os.environ['PATH']}
                    terminal = Terminal(shell, env, directory)
                    try:
                        self.assertIn(b'JJ_TEST_READY# ', terminal.output)
                        terminal.cmd('true', 'COMMAND_FINISHED')
                    finally:
                        terminal.close()

    def test_delayed_fish_startup_does_not_consume_command_deadline(self):
        fish = shutil.which('fish')
        self.assertIsNotNone(fish)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            wrapper = root / 'fish'
            # This is intentionally longer than the existing4s command deadline.
            wrapper.write_text('#!/bin/sh\nsleep 4.5\nexec ' + shlex.quote(fish) + ' "$@"\n')
            wrapper.chmod(0o700)
            env = {'HOME': directory, 'PATH': directory + ':' + os.environ['PATH']}
            before = time.monotonic()
            terminal = Terminal('fish', env, directory)
            try:
                self.assertGreaterEqual(time.monotonic() - before, 4.5)
                terminal.cmd('true', 'COMMAND_FINISHED')
            finally:
                terminal.close()

    def test_never_ready_shell_is_reported_and_reaped(self):
        with tempfile.TemporaryDirectory() as directory:
            wrapper = Path(directory) / 'fish'
            wrapper.write_text('#!/bin/sh\nexec /bin/sleep 30\n')
            wrapper.chmod(0o700)
            env = {'HOME': directory, 'PATH': directory + ':' + os.environ['PATH']}
            terminal = Terminal.__new__(Terminal)
            with self.assertRaisesRegex(AssertionError, 'shell startup timed out'):
                terminal.__init__('fish', env, directory, startup_timeout=0.3)
            with self.assertRaises(ChildProcessError):
                os.waitpid(terminal.pid, os.WNOHANG)
            with self.assertRaises(OSError):
                os.fstat(terminal.fd)


class ProcessCleanup(unittest.TestCase):
    def test_process_terminal_close_reaps_child(self):
        from test_pty import Terminal as ProcessTerminal
        with tempfile.TemporaryDirectory() as directory:
            terminal = ProcessTerminal(['/bin/sleep', '30'],
                                       {'PATH': os.environ['PATH']}, directory)
            terminal.close()
            with self.assertRaises(ChildProcessError):
                os.waitpid(terminal.pid, os.WNOHANG)
            with self.assertRaises(OSError):
                os.fstat(terminal.fd)
