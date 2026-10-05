"""Iteration 0027 shell-integration regressions.

AC-001: after sourcing the generated Zsh integration, Tab on an ordinary line and
on a `j <query> ` line behaves as the underlying widget, for repeated presses in
the same session.

The defect was the widget-name capture `${$(bindkey '^I')##* }`. In Zsh the
modifier is not applied to a command substitution nested in `${...}`, so the whole
`"^I" expand-or-complete` string was stored and the fallback ran
`zle "\"^I\" expand-or-complete"`, printing
`No such widget `"^I" expand-or-complete'` on every ordinary Tab press.

These tests drive real shells in a real PTY, modelled on tests/product/test_pty.py.
"""
import errno, os, pathlib, pty, select, shlex, signal, sqlite3, subprocess, tempfile, time, unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = pathlib.Path(os.environ.get('JJ_TEST_BIN', ROOT / 'target/debug/jjump')).resolve()
SHELL_ARGV = {
    'bash': ['bash', '--noprofile', '--norc', '-i'],
    'zsh': ['zsh', '-f', '-i'],
    'fish': ['fish', '--no-config', '--interactive'],
}


class Terminal:
    def __init__(self, argv, env, cwd):
        self.pid, self.fd = pty.fork()
        self.output = b''
        if self.pid == 0:
            os.chdir(cwd)
            os.execvpe(argv[0], argv, env)

    def send(self, text):
        os.write(self.fd, text.encode())

    def drain(self, seconds=0.3):
        end = time.monotonic() + seconds
        buf = b''
        while time.monotonic() < end:
            if select.select([self.fd], [], [], 0.02)[0]:
                try:
                    data = os.read(self.fd, 65536)
                except OSError as e:
                    if e.errno == errno.EIO:
                        break
                    raise
                if not data:
                    break
                buf += data
        self.output += buf
        return buf

    def until(self, needle, seconds=5):
        buf = b''
        end = time.monotonic() + seconds
        while needle not in buf and time.monotonic() < end:
            buf += self.drain(0.05)
        if needle not in buf:
            raise AssertionError((needle, buf[-2000:]))
        return buf

    def close(self):
        try:
            os.kill(self.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            os.waitpid(self.pid, os.WNOHANG)
        except ChildProcessError:
            pass
        os.close(self.fd)


class Fixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name).resolve()
        self.home = self.root / 'home'
        self.home.mkdir()
        self.alpha = self.home / 'alpha space 中文'
        self.beta = self.home / 'beta'
        self.alpha.mkdir()
        self.beta.mkdir()
        self.env = dict(os.environ, J_JUMP_PICKER='numbered', HOME=str(self.home),
                        PS1='JJ> ', PS2='MORE> ', NO_COLOR='1',
                        PATH=str(BIN.parent) + ':' + os.environ['PATH'])
        self.env.pop('TYPESAFE_API_KEY', None)
        self.env.pop('J_JUMP_CONFIG', None)
        for d in (self.alpha, self.beta):
            subprocess.run([str(BIN), 'record', '--', str(d)], env=self.shell_env('zsh'), cwd=d,
                           capture_output=True, check=True)

    def tearDown(self):
        self.tmp.cleanup()

    def shell_env(self, shell):
        return dict(self.env, J_JUMP_HOME=str(self.root / ('state-' + shell)))

    def init_file(self, shell, name=None):
        # These cases exercise configured navigation; fresh setup has its own PTYs.
        subprocess.run([str(BIN), "config", "set", "semantic", "off"],
                       env=self.shell_env(shell), capture_output=True, check=True)
        path = self.root / ('init-' + shell + '.txt')
        args = [str(BIN), 'init', shell] + (['--cmd', name] if name else [])
        path.write_bytes(subprocess.run(args, env=self.shell_env(shell), cwd=self.home,
                                        capture_output=True, check=True).stdout)
        return path

    def run_script(self, shell, script, name=None):
        init = self.init_file(shell, name)
        argv = SHELL_ARGV[shell][:-1] + ['-c', script]
        # Bash and Zsh consume the first operand as $0; Fish passes it straight to $argv.
        argv += [str(init)] if shell == 'fish' else ['test', str(init)]
        r = subprocess.run(argv, env=self.shell_env(shell), cwd=self.home,
                           capture_output=True, timeout=15)
        return init, r


class ZshTabCompletion(Fixture):
    """AC-001: Tab keeps working after the generated integration is sourced."""

    def zsh(self, prelude=None, init=None):
        init = init or self.init_file('zsh')
        t = Terminal(SHELL_ARGV['zsh'], dict(self.shell_env('zsh'), TERM='xterm'), self.home)
        try:
            if prelude:
                t.send(prelude + "\n")
                time.sleep(0.4)
                t.drain(0.3)
            t.send('source ' + shlex.quote(str(init)) + "; printf '\\nREADYZ\\n'\n")
            t.until(b'\r\nREADYZ\r\n')
            time.sleep(0.2)
        except Exception:
            t.close()
            raise
        return t

    def assert_no_widget_error(self, t):
        self.assertNotIn(b'No such widget', t.output, t.output[-2000:])

    def value(self, t, expression, marker):
        """Evaluate `$expression` in the live shell and return the printed value."""
        t.send("print -r -- '%s:'$%s; printf '\\nVALUEDONE\\n'\n" % (marker, expression))
        t.until(b'\r\nVALUEDONE\r\n')
        t.drain(0.2)
        chunk = t.output.rsplit(b'VALUEDONE', 1)[0].rsplit(marker.encode() + b':', 1)[1]
        return chunk.split(b'\r')[0].split(b'\n')[0].decode('utf-8', 'replace')

    def test_ordinary_line_tab_completes_and_never_reports_a_bad_widget(self):
        t = self.zsh()
        try:
            t.send('ls -d /tm\t')          # ordinary line: the fallback widget must run
            t.drain(0.4)
            self.assert_no_widget_error(t)
            t.send('\n')
            t.until(b'/tmp')               # completion produced /tmp/ before Enter
            self.assert_no_widget_error(t)
        finally:
            t.close()

    def test_repeated_tab_presses_in_one_session(self):
        t = self.zsh()
        try:
            t.send('echo ONE\t\t\n')       # two presses, then a second line
            t.until(b'ONE')
            t.send('echo TWO\t\t\n')
            t.until(b'TWO')
            self.assert_no_widget_error(t)
        finally:
            t.close()

    def test_j_line_tab_opens_picker_and_only_edits_the_buffer(self):
        t = self.zsh()
        try:
            t.send('j alpha \t')
            t.until(b'Choose a directory')
            t.send('1\r')
            t.until(b'j -- ')
            self.assert_no_widget_error(t)
            t.send('\x03')                 # cancel: no selection, no cd
            t.drain(0.3)
            t.send("printf '\\nSTAY:%s\\n' \"$PWD\"\n")
            t.until(('STAY:' + str(self.home)).encode())
            self.assert_no_widget_error(t)
        finally:
            t.close()

    def test_previous_tab_is_captured_as_a_bare_widget_name(self):
        t = self.zsh()
        try:
            # The defect stored the whole `"^I" expand-or-complete` string here.
            captured = self.value(t, '__jj_previous_tab', 'PREVTAB')
            self.assertEqual(captured, 'expand-or-complete')
        finally:
            t.close()

    def test_double_source_is_idempotent(self):
        init = self.init_file('zsh')
        source = 'source ' + shlex.quote(str(init))
        t = Terminal(SHELL_ARGV['zsh'], dict(self.shell_env('zsh'), TERM='xterm'), self.home)
        try:
            t.send(source + "; printf '\\nREADY1\\n'\n")
            t.until(b'\r\nREADY1\r\n')
            time.sleep(0.2)
            t.send(source + "; printf '\\nREADY2\\n'\n")
            t.until(b'\r\nREADY2\r\n')
            time.sleep(0.2)
            self.assertEqual(self.value(t, '__jj_previous_tab', 'PREVTAB'), 'expand-or-complete')
            self.assertEqual(self.value(t, '{#${(M)precmd_functions:#__jj_prompt}}', 'HOOKS'), '1')
            self.assertEqual(self.value(t, "(bindkey '^I')", 'BIND'), '"^I" __jj_complete_widget')
            t.send('ls -d /tm\t')          # still works after re-sourcing
            t.drain(0.4)
            self.assert_no_widget_error(t)
        finally:
            t.close()

    def test_other_tab_bindings_are_restored_or_safely_replaced(self):
        cases = [
            ('custom widget',
             "zle -N __jj_probe_tab; __jj_probe_tab() { LBUFFER+='CUSTOM!'; }; bindkey '^I' __jj_probe_tab"),
            ('unbound key', "bindkey -r '^I'"),
            ('macro text', "bindkey '^I' 'hello world'"),
        ]
        for label, prelude in cases:
            with self.subTest(binding=label):
                t = self.zsh(prelude=prelude)
                try:
                    captured = self.value(t, '__jj_previous_tab', 'PREVTAB')
                    # A bare widget name; never the raw `"^I" <binding>` string.
                    self.assertTrue(captured, label)
                    self.assertNotIn('"', captured)
                    self.assertNotIn(' ', captured)
                    if label == 'custom widget':
                        self.assertEqual(captured, '__jj_probe_tab')
                    t.send('ls -d /tm\t')
                    t.drain(0.4)
                    self.assert_no_widget_error(t)
                    if label == 'custom widget':
                        self.assertIn(b'CUSTOM!', t.output)
                finally:
                    t.close()


class ShellIntegrationContract(Fixture):
    """Behaviour the Zsh repair must not regress, checked in the same style."""

    def test_conflicting_command_name_is_refused_for_every_shell(self):
        scripts = {
            'bash': 'j() { :; }; source "$1"; rc=$?; type -t __jj_resolve >/dev/null && exit 99; exit "$rc"',
            'zsh': 'ji() { :; }; source "$1"; rc=$?; (( $+functions[__jj_resolve] )) && exit 99; exit "$rc"',
            'fish': 'function j; end; source $argv[1]; set rc $status; functions -q __jj_resolve; and exit 99; exit $rc',
        }
        for shell, script in scripts.items():
            with self.subTest(shell=shell):
                _, r = self.run_script(shell, script)
                self.assertEqual(r.returncode, 2, (shell, r.stdout, r.stderr))
                self.assertIn(b'--cmd', r.stderr)

    def test_custom_command_name_defines_only_that_pair(self):
        scripts = {
            'bash': 'source "$1"; type -t zzq >/dev/null && echo ZZQ-FN; '
                    'type -t zzqi >/dev/null && echo ZZQI-FN; type -t j >/dev/null || echo NO-J',
            'zsh': 'source "$1"; (( $+functions[zzq] )) && echo ZZQ-FN; '
                   '(( $+functions[zzqi] )) && echo ZZQI-FN; (( $+functions[j] )) || echo NO-J',
            'fish': 'source $argv[1]; functions -q zzq; and echo ZZQ-FN; '
                    'functions -q zzqi; and echo ZZQI-FN; functions -q j; or echo NO-J',
        }
        for shell, script in scripts.items():
            with self.subTest(shell=shell):
                _, r = self.run_script(shell, script, name='zzq')
                self.assertEqual(r.returncode, 0, (shell, r.stdout, r.stderr))
                self.assertIn(b'ZZQ-FN', r.stdout)
                self.assertIn(b'ZZQI-FN', r.stdout)
                self.assertIn(b'NO-J', r.stdout)

    def test_bash_prompt_command_survives_as_string_and_as_array(self):
        init = self.init_file('bash')
        source = 'source ' + shlex.quote(str(init))
        for label, setup, expected in [
            ('string', 'PROMPT_COMMAND="echo STRING-HOOK"', b'"__jj_prompt;echo STRING-HOOK"'),
            ('array', 'PROMPT_COMMAND=(a_hook b_hook)', b'declare -a PROMPT_COMMAND'),
        ]:
            with self.subTest(kind=label):
                r = subprocess.run(['bash', '--noprofile', '--norc', '-i', '-c',
                                    setup + '; ' + source + '; ' + source + '; declare -p PROMPT_COMMAND'],
                                   env=self.env, cwd=self.home, capture_output=True, timeout=15)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertIn(expected, r.stdout)
        # The array form keeps every pre-existing entry and adds __jj_prompt once.
        r = subprocess.run(['bash', '--noprofile', '--norc', '-i', '-c',
                            'PROMPT_COMMAND=(a_hook b_hook); ' + source + '; ' + source +
                            '; printf "JOINED=%s\\n" "${PROMPT_COMMAND[*]}"'],
                           env=self.env, cwd=self.home, capture_output=True, timeout=15)
        self.assertIn(b'JOINED=__jj_prompt a_hook b_hook', r.stdout)

    def test_sourcing_keeps_path_and_cd_untouched(self):
        scripts = {
            'bash': 'p0=$PATH; source "$1"; type -t cd; type -t j; '
                    '[ "$p0" = "$PATH" ] && echo PATH-SAME || echo PATH-CHANGED',
            'zsh': 'p0=$PATH; source "$1"; whence -w cd; whence -w j; '
                   '[[ $p0 == $PATH ]] && echo PATH-SAME || echo PATH-CHANGED',
            'fish': 'set p0 (string join : $PATH); source $argv[1]; '
                    'functions -q j; and echo J-FN; or echo J-NOT-FN; '
                    'functions cd | string match -q "*__jj*"; and echo CD-OVERRIDDEN; or echo CD-UNTOUCHED; '
                    'test "$p0" = (string join : $PATH); and echo PATH-SAME; or echo PATH-CHANGED',
        }
        for shell, script in scripts.items():
            with self.subTest(shell=shell):
                _, r = self.run_script(shell, script)
                self.assertEqual(r.returncode, 0, (shell, r.stdout, r.stderr))
                self.assertIn(b'PATH-SAME', r.stdout)
                self.assertNotIn(b'CD-OVERRIDDEN', r.stdout)
                if shell == 'bash':
                    self.assertIn(b'builtin', r.stdout)
                    self.assertIn(b'function', r.stdout)
                if shell == 'zsh':
                    self.assertIn(b'cd: builtin', r.stdout)
                    self.assertIn(b'j: function', r.stdout)
                if shell == 'fish':
                    self.assertIn(b'J-FN', r.stdout)

    def test_command_name_validation_rejects_keywords_and_internals(self):
        for name in ['cd', 'if', 'return', 'x y', '__jj_resolve', '1abc', '']:
            with self.subTest(name=name):
                r = subprocess.run([str(BIN), 'init', 'zsh', '--cmd', name], env=self.env,
                                   cwd=self.home, capture_output=True, timeout=10)
                self.assertEqual(r.returncode, 2, (name, r.stdout, r.stderr))
                self.assertEqual(r.stdout, b'')
        for shell in SHELL_ARGV:
            with self.subTest(shell=shell):
                self.assertEqual(self.init_file(shell, name='zzq').stat().st_size > 0, True)

    def test_previous_directory_semantics_per_shell(self):
        scripts = {
            'bash': 'source "$1"; cd "$2"; cd "$3"; j -; printf "PREV:%s\\n" "$PWD"',
            'zsh': 'source "$1"; cd "$2"; cd "$3"; j -; printf "PREV:%s\\n" "$PWD"',
            'fish': 'source $argv[1]; cd $argv[2]; cd $argv[3]; j -; printf "PREV:%s\\n" $PWD; '
                    'cd $argv[2]; cd $argv[3]; prevd; printf "PREVD:%s\\n" $PWD; nextd; printf "NEXTD:%s\\n" $PWD',
        }
        for shell, script in scripts.items():
            with self.subTest(shell=shell):
                init = self.init_file(shell)
                argv = SHELL_ARGV[shell][:-1] + ['-c', script]
                argv += [str(init), str(self.alpha), str(self.beta)] if shell == 'fish' \
                    else ['test', str(init), str(self.alpha), str(self.beta)]
                r = subprocess.run(argv, env=self.shell_env(shell), cwd=self.home,
                                   capture_output=True, timeout=15)
                self.assertEqual(r.returncode, 0, (shell, r.stdout, r.stderr))
                self.assertIn(('PREV:' + str(self.alpha)).encode(), r.stdout)
                if shell == 'fish':
                    self.assertIn(('PREVD:' + str(self.alpha)).encode(), r.stdout)
                    self.assertIn(('NEXTD:' + str(self.beta)).encode(), r.stdout)

    @unittest.skipUnless(os.environ.get('JJ_TEST_BIN'), 'rapid observer timing needs an optimized installed archive')
    def test_rapid_changed_prompts_count_both_directories_in_every_shell(self):
        """iteration0023 expectation: alpha=2 and beta=2 for the documented scenario."""
        for shell, argv in SHELL_ARGV.items():
            with self.subTest(shell=shell):
                state = self.root / ('rapid-' + shell)
                env = dict(self.shell_env(shell), J_JUMP_HOME=str(state),
                           TERM='dumb' if shell == 'fish' else 'xterm')
                init = self.init_file(shell)
                t = Terminal(argv, env, self.alpha)
                try:
                    t.send('source ' + shlex.quote(str(init)) + "; printf '\\nRAPID-READY\\n'\n")
                    t.until(b'\r\nRAPID-READY\r\n')
                    time.sleep(0.3)
                    t.send('cd ' + shlex.quote(str(self.beta)) + '; cd ' + shlex.quote(str(self.alpha))
                           + '; cd ' + shlex.quote(str(self.beta)) + "; printf '\\nBATCHED\\n'\n")
                    t.until(b'\r\nBATCHED\r\n')
                    time.sleep(0.3)
                    t.send('cd ' + shlex.quote(str(self.alpha)) + '\ncd '
                           + shlex.quote(str(self.beta)) + "; printf '\\nRAPID-END\\n'\n")
                    t.until(b'\r\nRAPID-END\r\n')
                    time.sleep(0.4)
                finally:
                    t.close()
                rows = dict(sqlite3.connect(state / 'data/visits.db')
                            .execute('select path,count from visits'))
                self.assertEqual(rows, {str(self.alpha): 2, str(self.beta): 2})


if __name__ == '__main__':
    unittest.main()
