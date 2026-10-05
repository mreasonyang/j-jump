"""User tasks for 0032. Synthetic state, real PTYs, no live provider or OS key store."""
import contextlib
import fcntl
import json
import os
import pathlib
import shlex
import shutil
import socket
import sqlite3
import struct
import subprocess
import tempfile
import termios
import threading
import time
import unittest

from test_pty import Terminal as ProcessTerminal
from test_navigation_fixes import Terminal as ShellTerminal

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = pathlib.Path(os.environ.get('JJ_TEST_BIN', ROOT / 'target/debug/jjump')).resolve()


class UXTasks(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name).resolve()
        self.home = self.root / 'home'
        self.cwd = self.home / 'neutral'
        self.target = self.home / 'alpha 中文'
        self.cwd.mkdir(parents=True)
        self.target.mkdir()
        self.state = self.root / 'state'
        self.env = dict(HOME=str(self.home), J_JUMP_HOME=str(self.state),
                        PATH=str(BIN.parent) + ':' + os.environ['PATH'],
                        LANG='en_US.UTF-8', LC_ALL='en_US.UTF-8', TERM='xterm',
                        NO_COLOR='1', J_JUMP_PICKER='numbered', PS1='UX> ')

    def tearDown(self):
        self.tmp.cleanup()

    def cli(self, *args, code=0, cwd=None, extra=None):
        env = dict(self.env, PWD=str(cwd or self.cwd))
        env.update(extra or {})
        result = subprocess.run([str(BIN), *map(str, args)], env=env,
                                cwd=cwd or self.cwd, capture_output=True, timeout=8)
        self.assertEqual(result.returncode, code, (args, result.stdout, result.stderr))
        return result

    def config(self):
        return self.state / 'config/config.json'

    def seed(self):
        self.cli('record', '--', self.target, cwd=self.target)

    @contextlib.contextmanager
    def process(self, *args, cols=80):
        t = ProcessTerminal([str(BIN), *args], self.env, self.cwd)
        fcntl.ioctl(t.fd, termios.TIOCSWINSZ, struct.pack('HHHH', 12, cols, 0, 0))
        try:
            yield t
        finally:
            t.close()

    @contextlib.contextmanager
    def shell(self, name, cmd='j'):
        init = self.root / (name + '.init')
        init.write_bytes(self.cli('init', name, '--cmd', cmd).stdout)
        t = ShellTerminal(name, self.env, self.cwd)
        try:
            t.cmd('source ' + shlex.quote(str(init)), 'READY')
            yield t
        finally:
            t.close()

    def test_help_does_not_cd_and_uses_custom_names(self):
        self.cli('config', 'set', 'tracking', 'off')
        for shell in ('bash', 'zsh', 'fish'):
            for name in ('j', 'ski'):
                with self.subTest(shell=shell, name=name), self.shell(shell, name) as t:
                    for command in (name, name + 'i'):
                        start = len(t.output)
                        # Fish uses $status, POSIX adapters use $?.
                        status = '$status' if shell == 'fish' else '$?'
                        t.cmd(command + ' --help; printf "RC:%s\\n" ' + status, 'HELPDONE')
                        out = t.output[start:]
                        self.assertIn(b'RC:0', out)
                        self.assertIn(b'jjump setup', out)
                        self.assertNotIn(b'\njj ', out)
                        self.assertIn(('Directory navigation: ' + command).encode(), out)
                        if command == name:
                            self.assertIn((name + ' -').encode(), out)
                        t.cmd('printf "WHERE:%s\\n" "$PWD"', 'WHERECHECK')
                        self.assertIn(('WHERE:' + str(self.cwd)).encode(), t.output)

    def test_literal_help_directory_is_still_a_path(self):
        self.cli('config', 'set', 'tracking', 'off')
        target = self.cwd / '--help'
        target.mkdir()
        for shell in ('bash', 'zsh', 'fish'):
            with self.subTest(shell=shell), self.shell(shell) as t:
                t.cmd('j -- --help; printf "WHERE:%s\\n" "$PWD"', 'LITERAL')
                self.assertIn(('WHERE:' + str(target)).encode(), t.output)

    def test_help_lists_settings_and_correct_shell_examples(self):
        out = self.cli('--help').stdout
        for shell in ('bash', 'zsh', 'fish'):
            self.assertIn(('jjump init ' + shell).encode(), out)
        self.assertNotIn(b'Bash/zsh:', out)
        settings = self.cli('config', 'set', '--help').stdout
        for key in ('language', 'privacy', 'consent', 'candidate_limit', 'exclude', 'no_send'):
            self.assertIn(key.encode(), settings)

    def test_invalid_setting_names_field_preserves_bytes(self):
        self.cli('config', 'set', 'tracking', 'off')
        before = self.config().read_bytes()
        for key, value in [('privacy', 'private'), ('consent', 'yes'),
                           ('candidate_limit', '255'), ('language', 'xx')]:
            with self.subTest(key=key):
                out = self.cli('config', 'set', key, value, code=2)
                self.assertIn(key.encode(), out.stderr)
                self.assertEqual(before, self.config().read_bytes())

    def test_human_output_and_json_are_separate_with_privacy(self):
        private = self.home / 'private-client'
        self.cli('config', 'set', 'exclude', json.dumps([str(private)]))
        human = self.cli('config', 'show').stdout
        machine = json.loads(self.cli('config', 'show', '--json').stdout)
        self.assertFalse(human.startswith(b'{'))
        self.assertNotIn(str(private).encode(), human)
        self.assertEqual(machine['saved']['exclude'], [str(private)])
        self.assertFalse(self.cli('doctor').stdout.startswith(b'{'))
        status = json.loads(self.cli('doctor', '--json').stdout)
        self.assertEqual(status['configuration'], 'ok')
        self.assertEqual(status['provider_test'], 'not run')

    def test_recovery_advice_is_actionable_and_preview_explicit(self):
        self.seed()
        self.cli('config', 'set', 'tracking', 'off')
        self.config().write_text('{broken')
        advice = self.cli('doctor', code=7).stdout
        self.assertIn(b'jjump config recover --apply', advice)
        preview = self.cli('config', 'recover').stdout
        self.assertIn(b'Preview only; nothing changed', preview)
        self.assertEqual(self.config().read_text(), '{broken')
        applied = self.cli('config', 'recover', '--apply').stdout
        self.assertIn(b'Recovery complete', applied)
        cfg = json.loads(self.config().read_text())
        self.assertFalse(cfg['semantic'])
        self.assertFalse(cfg['tracking'])
        self.assertEqual(self.cli('query', 'alpha').stdout.decode().strip(), str(self.target))

    def test_reset_preview_and_applied_status_differ(self):
        self.cli('config', 'set', 'semantic', 'on')
        self.assertIn(b'Preview only', self.cli('config', 'reset').stdout)
        self.assertTrue(json.loads(self.config().read_text())['semantic'])
        self.assertIn(b'Reset complete', self.cli('config', 'reset', '--apply').stdout)
        self.assertFalse(json.loads(self.config().read_text())['semantic'])

    def test_setup_invalid_input_retry_back_and_save(self):
        with self.process('setup') as t:
            t.until(b'Semantic provider'); t.send('jve\n')
            t.until(b'choose off or jev'); t.send('jev\n')
            t.until(b'Network permission'); t.send('always\n')
            t.until(b'Fields'); t.send('invalid\n')
            t.until(b'privacy expects'); t.send('b\n')
            t.until(b'Network permission'); t.send('ask\n')
            t.until(b'Fields'); t.send('strict\n')
            t.until(b'API key ['); t.send('skip\n')
            t.until(b'Local visit tracking'); t.send('off\n')
            t.until(b'Advanced settings'); t.send('\n')
            t.until(b'Saved.')
            # Readiness is written after Saved; PTY reads need not combine them.
            if b'Jev blocked: no credential configured' not in t.output:
                t.until(b'Jev blocked: no credential configured')
            self.assertIn(b'Jev blocked: no credential configured', t.output)
            self.assertNotIn(b'Save this configuration', t.output)
        cfg = json.loads(self.config().read_text())
        self.assertTrue(cfg['semantic']); self.assertFalse(cfg['tracking'])
        self.assertEqual(cfg['consent'], 'ask'); self.assertEqual(cfg['privacy'], 'strict')

    def test_setup_focused_edit_and_discard(self):
        self.cli('config', 'set', 'candidate_limit', '12')
        before = self.config().read_bytes()
        with self.process('setup') as t:
            t.until(b'q Exit without saving'); t.send('2\n')
            t.until(b'Local visit tracking'); t.send('off\n')
            t.until(b'q Exit without saving'); t.send('q\n'); t.cancelled()
        self.assertEqual(before, self.config().read_bytes())
        with self.process('setup') as t:
            t.until(b'q Exit without saving'); t.send('2\n')
            t.until(b'Local visit tracking'); t.send('off\n')
            t.until(b'q Exit without saving'); t.send('s\n')
            t.until(b'Saved.')
        cfg = json.loads(self.config().read_text())
        self.assertFalse(cfg['tracking']); self.assertEqual(cfg['candidate_limit'], 12)

    def test_setup_back_between_steps_retains_draft(self):
        with self.process('setup') as t:
            t.until(b'Semantic provider'); t.send('off\n')
            t.until(b'Local visit tracking'); t.send('off\n')
            t.until(b'Advanced settings'); t.send('b\n')
            t.until(b'Local visit tracking'); t.send('\n')
            t.until(b'Advanced settings'); t.send('\n')
            t.until(b'Saved.')
            self.assertNotIn(b'Save this configuration', t.output)
        self.assertFalse(json.loads(self.config().read_text())['tracking'])

    def test_setup_plain_root_input_and_atomic_concurrent_save(self):
        self.cli('config', 'set', 'tracking', 'off')
        with self.process('setup') as t:
            t.until(b'q Exit without saving'); t.send('3\n')
            t.until(b'Enter: done'); t.send(str(self.home / 'secret') + '\n')
            t.until(b'Enter: done'); t.send('\n')
            t.until(b'Enter: done'); t.send('\n')
            t.until(b'Candidate limit'); t.send('\n')
            t.until(b'Language'); t.send('\n')
            t.until(b'q Exit without saving')
            self.cli('config', 'set', 'candidate_limit', '17')
            before = self.config().read_bytes()
            t.send('s\n'); t.until(b'configuration changed concurrently')
        self.assertEqual(before, self.config().read_bytes())

    def test_locale_saved_language_override_and_json_stability(self):
        self.assertIn('本地目录导航'.encode(), self.cli('--help', extra={'LC_ALL': 'zh_CN.UTF-8'}).stdout)
        self.cli('config', 'set', 'language', 'zh')
        self.assertIn('本地目录导航'.encode(), self.cli('--help').stdout)
        self.assertIn(b'Private local', self.cli('--help', extra={'J_JUMP_LANG': 'en'}).stdout)
        self.assertIn('配置文件'.encode(), self.cli('config', 'show').stdout)
        self.assertIn('仅预览'.encode(), self.cli('config', 'recover').stdout)
        self.assertIn('输入无效'.encode(), self.cli('config', 'set', 'privacy', 'bad', code=2).stderr)
        self.assertEqual(json.loads(self.cli('config', 'show', '--json').stdout)['saved']['language'], 'zh')
        self.cli('config', 'set', 'language', 'auto')
        self.assertIn(b'Private local', self.cli('--help', extra={'LC_ALL': 'xx_TEST'}).stdout)

    def test_chinese_setup_and_picker_40_columns_without_color(self):
        self.env['LC_ALL'] = 'zh_CN.UTF-8'
        with self.process('setup', cols=40) as t:
            t.until('语义服务'.encode()); t.send('off\n')
            t.until('本地访问记录'.encode()); t.send('on\n')
            t.until('高级设置'.encode()); t.send('q\n'); t.cancelled()
            self.assertNotIn(b'\x1b[', t.output)
        self.assertFalse(self.config().exists())
        self.seed()
        with self.process('query', '--interactive', cols=40) as t:
            t.until('选择目录'.encode()); t.send('\n'); t.cancelled()
            self.assertNotIn(b'\x1b[', t.output)

    def test_numbered_pagination_inspection_retry_and_selection(self):
        self.seed()
        for i in range(23):
            p = self.home / ('shared-prefix-' * 4 + str(i))
            p.mkdir(); self.cli('record', '--', p, cwd=p)
        with self.process('query', '--interactive', cols=40) as t:
            first = t.until(b'> ')
            self.assertIn(b'Page 1/', first)
            # 12 rows -> seven entries plus headers, not the entire inventory.
            self.assertNotIn(b'8. ', first)
            t.send('999\n'); t.until(b'Enter a number in'); t.send('n\n')
            second = t.until(b'Page 2/'); self.assertIn(b'Page 2/', second)
            t.send('p\n'); t.until(b'Page 1/'); t.send('v 1\n')
            t.until(str(self.home).encode()); t.send('1\n')
            t.until(str(self.home).encode())

    def test_query_miss_reports_no_match_without_broadening(self):
        self.seed()
        with self.process('query', '--interactive', 'alhpa') as t:
            t.until(b'J3:')
            self.assertNotIn(b'Choose a directory', t.output)
            self.assertNotIn(str(self.target).encode(), t.output)

    def test_init_and_nonterminal_setup_check_do_not_create_configuration(self):
        self.cli('init', 'zsh')
        self.cli('setup-if-needed')
        self.assertFalse(self.config().exists())
        self.assertFalse(pathlib.Path(str(self.config()) + '.notice').exists())

    def test_plain_root_editor_retries_and_saves_without_json(self):
        self.cli('config', 'set', 'tracking', 'off')
        private = self.home / 'private project'
        with self.process('setup') as t:
            t.until(b'q Exit without saving'); t.send('3\n')
            t.until(b'Enter: done'); t.send('relative\n')
            t.until(b'roots must be absolute'); t.send(str(private) + '\n')
            t.until(b'Enter: done'); t.send('\n')
            t.until(b'Enter: done'); t.send('\n')
            t.until(b'Candidate limit'); t.send('\n')
            t.until(b'Language'); t.send('\n')
            t.until(b'q Exit without saving'); t.send('s\n')
            t.until(b'Saved.')
        self.assertEqual(json.loads(self.config().read_text())['exclude'], [str(private)])

    def test_chinese_fzf_uses_saved_language(self):
        if not shutil.which('fzf'):
            self.skipTest('optional fzf unavailable')
        self.seed()
        self.cli('config', 'set', 'tracking', 'off')
        self.cli('config', 'set', 'language', 'zh')
        self.env['J_JUMP_PICKER'] = 'fzf'
        with self.shell('zsh') as t:
            t.send('ji\r'); t.until('目录> '.encode()); t.drain(.15)
            t.send('alpha'); t.drain(.15); t.send('\r'); t.drain(.25)
            t.cmd('printf "WHERE:%s\\n" "$PWD"', 'CHINESE')
            self.assertIn(('WHERE:' + str(self.target)).encode(), t.output)

    @contextlib.contextmanager
    def delayed_proxy(self):
        server = socket.socket()
        server.bind(('127.0.0.1', 0)); server.listen(); server.settimeout(.1)
        stop = threading.Event(); connections = []; accepted = []
        def hold():
            while not stop.is_set():
                try:
                    conn, _ = server.accept(); accepted.append(conn); connections.append(True)
                except socket.timeout:
                    pass
        worker = threading.Thread(target=hold); worker.start()
        old = self.env.copy()
        proxy = 'http://127.0.0.1:' + str(server.getsockname()[1])
        self.env.update(TYPESAFE_API_KEY='synthetic-not-a-real-key', HTTPS_PROXY=proxy,
                        HTTP_PROXY=proxy, ALL_PROXY=proxy, NO_PROXY='')
        try:
            yield connections
        finally:
            self.cli('adapter', 'stop')
            stop.set(); worker.join(timeout=2)
            for conn in accepted:
                conn.close()
            server.close(); self.env = old

    def test_wait_enter_picker_all_shells_routes_select_and_cancel(self):
        for shell in ('bash', 'zsh', 'fish'):
            for mode in ('ordinary', 'interactive', 'forced', 'completion'):
                for choose in (False, True):
                    with self.subTest(shell=shell, mode=mode, choose=choose):
                        # Distinct circuits avoid carrying fixture failures into another task.
                        self.state = self.root / ('wait-' + shell + mode + str(choose))
                        self.env['J_JUMP_HOME'] = str(self.state)
                        self.seed()
                        for key, val in [('semantic', 'on'), ('consent', 'always'), ('tracking', 'off')]:
                            self.cli('config', 'set', key, val)
                        with self.delayed_proxy() as connections, self.shell(shell) as t:
                            command = {'ordinary': 'j notlexical\r', 'interactive': 'ji notlexical\r',
                                       'forced': 'j --force-semantic alpha\r', 'completion': 'j notlexical \t'}[mode]
                            t.send(command); t.until(b'[Enter=yes / W=wait]'); t.send('\r')
                            t.until(b'> ')
                            self.assertIn(b'Choose a directory', t.output)
                            # No completed cd before a separate picker answer.
                            self.assertNotIn(b'J3:', t.output)
                            t.send('1\r' if choose else '\r')
                            if mode == 'completion':
                                t.drain(.25)
                                if choose:
                                    self.assertIn(b'j -- ', t.output)
                                    # Completing only edits the buffer: cancelling that line stays put.
                                t.send('\x03'); t.drain(.1)
                            elif not choose:
                                t.cancelled()
                            else:
                                t.drain(.2)
                            t.cmd('printf "WHERE:%s\\n" "$PWD"', 'ENDTASK')
                            expected = self.target if choose and mode != 'completion' else self.cwd
                            self.assertIn(('WHERE:' + str(expected)).encode(), t.output)
                            self.assertEqual(len(connections), 1)
                            with sqlite3.connect(self.state / 'cache/semantic-cache.db') as db:
                                self.assertEqual(db.execute('select count(*) from dispatch').fetchone()[0], 1)


if __name__ == '__main__':
    unittest.main()
