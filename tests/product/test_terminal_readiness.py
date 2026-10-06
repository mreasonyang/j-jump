"""Shell acceptance must observe editor readiness rather than guess a delay."""
import os
from pathlib import Path
import shlex
import tempfile
import time
import unittest

from test_navigation_fixes import Terminal


class TerminalReadiness(unittest.TestCase):
    def test_followup_waits_for_slow_foreground_terminal_restoration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            program = root / 'slow-terminal-return.py'
            program.write_text(
                'import os, termios, time\n'
                'fd = os.open("/dev/tty", os.O_RDWR)\n'
                'os.write(fd, b"FOREGROUND_DONE\\r\\n")\n'
                'time.sleep(.4)\n'
                'termios.tcflush(fd, termios.TCIFLUSH)\n'
                'os.close(fd)\n'
            )
            env = {'HOME': directory, 'PATH': os.environ['PATH'],
                   'LANG': 'C.UTF-8', 'NO_COLOR': '1'}
            for shell in ('bash', 'zsh', 'fish'):
                with self.subTest(shell=shell):
                    t = Terminal(shell, env, root)
                    try:
                        started = time.monotonic()
                        output = t.send_and_wait_prompt(
                            'python3 ' + shlex.quote(str(program)) + '\r')
                        self.assertIn(b'FOREGROUND_DONE', output)
                        self.assertGreaterEqual(time.monotonic() - started, .4)
                        t.cmd('printf "FOLLOWUP_OK\\n"', 'FOLLOWUP_COMPLETE')
                        self.assertIn(b'FOLLOWUP_OK\r\n', t.output)
                    finally:
                        t.close()
