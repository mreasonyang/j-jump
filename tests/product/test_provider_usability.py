"""Bilingual user journeys; synthetic profiles and local adapter replies only."""
import contextlib
import fcntl
import json
import os
import subprocess
import struct
import termios
import unittest

import test_0036_first_use as first
import test_0033_current_contract as current
from test_pty import Terminal
from test_0037_setup_keys import FIXTURE

ACCOUNT = '00000000000000000000000000000001'
SECRET = 'synthetic-usability-token'


class SetupUsability(unittest.TestCase):
    setUp = first.FirstUse.setUp
    profile = first.FirstUse.profile
    cli = first.FirstUse.cli

    @contextlib.contextmanager
    def setup(self, fixture=False):
        t = Terminal([str(FIXTURE if fixture else first.BIN), 'setup'], self.env, self.cwd)
        fcntl.ioctl(t.fd, termios.TIOCSWINSZ, struct.pack('HHHH', 12, 40, 0, 0))
        try:
            yield t
        finally:
            self.assertNotIn(SECRET.encode(), t.output)
            t.close()

    def select(self, t, provider, language):
        t.until(('Semantic provider' if language == 'en' else '语义服务').encode())
        t.send(provider.upper() + '\r')
        t.until(('Network permission' if language == 'en' else '联网许可').encode())
        t.send('\r')
        t.until(('Fields' if language == 'en' else '发送字段').encode())
        t.send('\r')
        if provider == 'clef-flash':
            t.until(b'Cloudflare Account ID [')
            self.assertIn(('32 hexadecimal characters' if language == 'en' else '32 位十六进制').encode(), t.output)
            t.send(ACCOUNT + '\r')
        t.until(('API key [' if language == 'en' else 'API Key [').encode())

    def test_setup_explains_skip_and_environment_precedence_for_both_providers(self):
        for provider in ('jev', 'clef-flash'):
            name = 'TYPESAFE_API_KEY' if provider == 'jev' else 'CLOUDFLARE_AUTH_TOKEN'
            for language in ('en', 'zh'):
                with self.subTest(provider=provider, language=language):
                    self.profile(provider + language)
                    self.env.update(J_JUMP_LANG=language)
                    self.env.pop('TYPESAFE_API_KEY', None)
                    self.env.pop('CLOUDFLARE_AUTH_TOKEN', None)
                    self.env[name] = SECRET
                    with self.setup() as t:
                        self.select(t, provider, language)
                        self.assertIn(('skip: stop using the stored key; environment keys still apply' if language == 'en' else
                                       'skip：不使用已保存的 Key；环境凭据仍会生效').encode(), t.output)
                        self.assertIn(('off to disable cloud requests' if language == 'en' else 'off 关闭云端请求').encode(), t.output)
                        t.send('skip\r')
                        t.until(('Local visit tracking' if language == 'en' else '本地访问记录').encode())
                        t.send('\r')
                        t.until(('Advanced settings' if language == 'en' else '高级设置').encode())
                        t.send('\r')
                        t.until(('Saved.' if language == 'en' else '已保存。').encode())
                    cfg = json.loads(self.config.read_text())
                    self.assertTrue(cfg['semantic'])
                    self.assertEqual(cfg['provider'], provider)
                    self.assertEqual(cfg['credential' if provider == 'jev' else 'cloudflare_credential'], 'environment')
                    self.assertTrue(json.loads(self.cli('doctor', '--json').stdout)['credential']['present'])

    def test_account_override_advice_has_a_working_recovery_path(self):
        self.env.pop('J_JUMP_OFFLINE')
        for language in ('en', 'zh'):
            with self.subTest(language=language):
                self.profile('account-' + language)
                self.env.update(J_JUMP_LANG=language, CLOUDFLARE_AUTH_TOKEN=SECRET,
                                CLOUDFLARE_ACCOUNT_ID='invalid-account-override')
                for key, value in (('provider', 'clef-flash'), ('semantic', 'on'), ('cloudflare_account_id', ACCOUNT)):
                    self.cli('config', 'set', key, value)
                before = self.config.read_bytes()
                output = self.cli('doctor').stdout
                self.assertIn(b'CLOUDFLARE_ACCOUNT_ID', output)
                self.assertIn(('unset it' if language == 'en' else '清除它').encode(), output)
                self.assertNotIn(b'invalid-account-override', output)
                self.assertFalse(json.loads(self.cli('doctor', '--json').stdout)['environment_ready'])
                # Follow the displayed advice; saved configuration needs no rewrite.
                self.env.pop('CLOUDFLARE_ACCOUNT_ID')
                self.assertTrue(json.loads(self.cli('doctor', '--json').stdout)['environment_ready'])
                self.assertEqual(self.config.read_bytes(), before)

    def test_invalid_token_environment_is_not_reported_ready_or_echoed(self):
        self.env.pop('J_JUMP_OFFLINE')
        for provider, name in (('jev', 'TYPESAFE_API_KEY'), ('clef-flash', 'CLOUDFLARE_AUTH_TOKEN'),
                               ('clef-flash', 'CLOUDFLARE_API_TOKEN')):
            for language in ('en', 'zh'):
                for value in (b'\xff', 'invalid\nsynthetic-token', 'x' * 4097):
                    with self.subTest(provider=provider, variable=name, language=language, invalid_utf8=isinstance(value, bytes)):
                        self.profile('invalid-' + name + language + str(len(value)))
                        for variable in ('TYPESAFE_API_KEY', 'CLOUDFLARE_AUTH_TOKEN', 'CLOUDFLARE_API_TOKEN'):
                            self.env.pop(variable, None)
                        self.env.update(J_JUMP_LANG=language)
                        self.env[name] = value
                        self.cli('config', 'set', 'provider', provider)
                        self.cli('config', 'set', 'cloudflare_account_id', ACCOUNT)
                        self.cli('config', 'set', 'semantic', 'on')
                        self.assertFalse(json.loads(self.cli('doctor', '--json').stdout)['environment_ready'])
                        output = self.cli('doctor').stdout
                        self.assertIn(name.encode(), output)
                        self.assertIn(('invalid' if language == 'en' else '无效').encode(), output)
                        self.assertNotIn(b'synthetic-token', output)
                        self.assertNotIn(b'\xff', output)
                        before = self.config.read_bytes()
                        t = Terminal([str(first.BIN), '--force-semantic', 'query', 'missing-directory'],
                                     self.env, self.cwd)
                        try:
                            t.until(b'J5:')
                            self.assertNotIn(b'Choose a directory', t.output)
                            self.assertNotIn('\r\n选择目录\r\n'.encode(), t.output)
                            self.assertNotIn(b'synthetic-token', t.output)
                        finally:
                            t.close()
                        self.assertFalse((self.state / 'cache/semantic-cache.db').exists())
                        self.env[name] = SECRET
                        self.assertTrue(json.loads(self.cli('doctor', '--json').stdout)['environment_ready'])
                        self.assertEqual(before, self.config.read_bytes())

    def test_chinese_invalid_provider_and_account_guidance_keeps_configuration(self):
        self.env['J_JUMP_LANG'] = 'zh'
        self.cli('config', 'set', 'tracking', 'off')
        before = self.config.read_bytes()
        for key, value, expected in (('provider', 'clef', '请选择 jev 或 clef-flash'),
                                     ('cloudflare_account_id', 'wrong', '32 位十六进制')):
            result = subprocess.run([str(first.BIN), 'config', 'set', key, value],
                                    env=self.env, cwd=self.cwd, capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 2)
            self.assertIn(expected.encode(), result.stderr)
            self.assertEqual(self.config.read_bytes(), before)

    def test_advanced_limit_guidance_matches_accepted_bounds_and_retains_draft(self):
        for provider in ('jev', 'clef-flash'):
            for language in ('en', 'zh'):
                with self.subTest(provider=provider, language=language):
                    self.profile('limit-' + provider + language)
                    self.env['J_JUMP_LANG'] = language
                    with self.setup() as t:
                        self.select(t, provider, language); t.send('skip\r')
                        t.until(('Local visit tracking' if language == 'en' else '本地访问记录').encode()); t.send('\r')
                        t.until(('Advanced settings' if language == 'en' else '高级设置').encode()); t.send('edit\r')
                        t.until(('Folders excluded from history and search' if language == 'en' else '不记录、不搜索的文件夹').encode())
                        t.send('\r')
                        t.until(('Folders whose directory information stays local' if language == 'en' else '不向云端发送目录信息的文件夹').encode())
                        t.send('\r')
                        label = ('Candidate limit' if language == 'en' else '候选数量').encode()
                        t.until(label)
                        self.assertIn(b'[1..254; 254]', t.output)
                        t.send('255\r'); t.until(b'J2:')
                        self.assertFalse(self.config.exists())
                        if label not in t.output[-100:]:
                            t.until(label)
                        t.send('65\r')
                        t.until(('Language' if language == 'en' else '显示语言').encode()); t.send('b\r')
                        t.until(label)
                        self.assertIn(b'[1..254; 65]', t.output)
                        t.send('254\r')
                        t.until(('Language' if language == 'en' else '显示语言').encode()); t.send('\r')
                        t.until(('Saved.' if language == 'en' else '已保存。').encode())
                    self.assertEqual(json.loads(self.config.read_text())['candidate_limit'], 254)

    def test_back_after_provider_switch_discards_key_draft_before_save(self):
        for source, destination in (('jev', 'clef-flash'), ('clef-flash', 'jev')):
            for language in ('en', 'zh'):
                with self.subTest(source=source, language=language):
                    self.profile('back-' + source + language)
                    self.env.update(J_JUMP_LANG=language)
                    for name in ('TYPESAFE_API_KEY', 'CLOUDFLARE_AUTH_TOKEN'):
                        self.env.pop(name, None)
                    for key, value in (('provider', source), ('cloudflare_account_id', ACCOUNT), ('semantic', 'on')):
                        self.cli('config', 'set', key, value)
                    menu = ('s Save; q Exit without saving' if language == 'en' else 's 保存；q 退出不保存').encode()
                    with self.setup(fixture=True) as t:
                        t.until(menu); t.send('6\r')
                        if source == 'clef-flash':
                            t.until(b'Cloudflare Account ID ['); t.send('\r')
                        t.until(('API key [' if language == 'en' else 'API Key [').encode()); t.send('enter\r')
                        t.until(('hidden; Enter' if language == 'en' else '隐藏输入').encode())
                        self.assertFalse(termios.tcgetattr(t.fd)[3] & termios.ECHO)
                        t.send(SECRET + '\r'); t.until(menu); t.send('1\r')
                        t.until(('Semantic provider' if language == 'en' else '语义服务').encode())
                        t.send(destination + '\r')
                        t.until(('Network permission' if language == 'en' else '联网许可').encode()); t.send('b\r')
                        t.until(('Semantic provider' if language == 'en' else '语义服务').encode()); t.send('b\r')
                        t.until(menu); t.send('s\r')
                        t.until(('Saved.' if language == 'en' else '已保存。').encode())
                        self.assertNotIn(b'FIXTURE: credential stored in memory', t.output)
                    cfg = json.loads(self.config.read_text())
                    self.assertEqual(cfg['provider'], destination)
                    self.assertEqual((cfg['credential'], cfg['cloudflare_credential']), ('environment', 'environment'))


class InteractionUsability(unittest.TestCase):
    setUp = current.CurrentContract.setUp
    tearDown = current.CurrentContract.tearDown
    cli = current.CurrentContract.cli
    config = current.CurrentContract.config
    seed = current.CurrentContract.seed
    shell = current.CurrentContract.shell
    process = current.CurrentContract.process
    runtime = current.CurrentContract.runtime
    recv_exact = staticmethod(current.CurrentContract.recv_exact)
    adapter_reply = current.CurrentContract.adapter_reply

    def configure(self, suffix, provider, language):
        current.CurrentContract.configure_semantics(self, suffix)
        self.env.update(J_JUMP_LANG=language)
        self.env.pop('CLOUDFLARE_AUTH_TOKEN', None)
        if provider == 'clef-flash':
            self.env.update(CLOUDFLARE_AUTH_TOKEN=SECRET, CLOUDFLARE_ACCOUNT_ID=ACCOUNT)
        self.cli('config', 'set', 'provider', provider)

    def where(self, t, path, marker):
        start = len(t.output)
        t.cmd('printf "WHERE:%s\\n" "$PWD"', marker)
        self.assertIn(os.fsencode('WHERE:' + str(path)), t.output[start:])

    def test_consent_discloses_actual_fields_and_decline_never_dispatches(self):
        for provider in ('jev', 'clef-flash'):
            for language in ('en', 'zh'):
                for privacy in ('strict', 'balanced', 'full'):
                    for shell in ('bash', 'zsh', 'fish'):
                        with self.subTest(provider=provider, language=language, privacy=privacy, shell=shell):
                            self.configure(provider + language + privacy + shell, provider, language)
                            self.cli('config', 'set', 'consent', 'ask')
                            self.cli('config', 'set', 'privacy', privacy)
                            with self.shell(shell) as t, self.adapter_reply('valid') as frames:
                                start = len(t.output)
                                t.send('ji notlexical\r'); t.until(b'[y/N]')
                                out = t.output[start:]
                                self.assertIn(('Send query' if language == 'en' else '发送查询').encode(), out)
                                self.assertIn(('Jev' if provider == 'jev' else 'Clef-Flash').encode(), out)
                                fields = {'strict': ('directory names', '目录名'),
                                          'balanced': ('parent names', '父目录名'),
                                          'full': ('full paths', '完整路径')}
                                self.assertIn(fields[privacy][language == 'zh'].encode(), out)
                                if privacy != 'strict':
                                    self.assertIn(('usage' if language == 'en' else '使用频度').encode(), out)
                                t.send_and_wait_prompt('\r'); t.cancelled()
                                self.assertEqual(frames, [])
                                self.where(t, self.cwd, 'DECLINED')
                                self.assertFalse((self.state / 'cache/semantic-cache.db').exists())
                                t.send('ji notlexical\r'); t.until(b'[y/N]'); t.send('y\r')
                                t.until(('Empty/q: cancel' if language == 'en' else '空输入/q 取消').encode())
                                self.assertIn(('suggestion: first; choose explicitly' if language == 'en' else
                                               '建议排在首位；请自行选择').encode(), t.output)
                                t.send_and_wait_prompt('q\r'); t.cancelled()
                                self.where(t, self.cwd, 'PICKER_CANCELLED')
                                self.assertEqual(frames, ['Send'])
                                self.assertNotIn(SECRET.encode(), t.output)

    def test_provider_errors_name_service_and_offer_local_recovery(self):
        for provider in ('jev', 'clef-flash'):
            for language in ('en', 'zh'):
                for outcome, en, zh in (('error', 'authentication rejected', '凭据'),
                                        ('invalid', 'invalid JSON', '响应格式'),
                                        ('rate_limit', 'rate limit', '配额'),
                                        ('timeout', 'timed out', '超时'),
                                        ('transport', 'transport failed', '连接失败')):
                    for shell in ('bash', 'zsh', 'fish'):
                        with self.subTest(provider=provider, language=language, outcome=outcome, shell=shell):
                            self.configure(provider + language + outcome + shell, provider, language)
                            with self.shell(shell) as t, self.adapter_reply(outcome) as frames:
                                out = t.send_and_wait_prompt('ji notlexical\r')
                                self.assertIn(b'J5:', out)
                                self.assertIn((en if language == 'en' else zh).encode(), out)
                                self.assertIn(b'jjump --offline query --interactive', out)
                                self.assertIn(('Jev' if provider == 'jev' else 'Clef-Flash').encode(), out)
                                self.assertNotIn('\r\n选择目录\r\n'.encode(), out)
                                self.where(t, self.cwd, 'ERROR_STOPPED')
                                self.assertEqual(frames, ['Send'])

    def test_invalid_cloud_credentials_preserve_local_and_previous_navigation(self):
        for provider in ('jev', 'clef-flash'):
            for language in ('en', 'zh'):
                for shell in ('bash', 'zsh', 'fish'):
                    with self.subTest(provider=provider, language=language, shell=shell):
                        self.configure('local-' + provider + language + shell, provider, language)
                        key = 'TYPESAFE_API_KEY' if provider == 'jev' else 'CLOUDFLARE_AUTH_TOKEN'
                        self.env[key] = 'invalid\nsynthetic-token'
                        if provider == 'clef-flash':
                            self.env['CLOUDFLARE_ACCOUNT_ID'] = 'invalid-account'
                        with self.shell(shell) as t:
                            t.cmd('j alpha', 'LOCAL_MATCH')
                            self.where(t, self.target, 'LOCAL_CWD')
                            t.cmd('j -', 'PREVIOUS')
                            self.where(t, self.cwd, 'PREVIOUS_CWD')
                            t.cmd('j --offline alpha', 'OFFLINE_MATCH')
                            self.where(t, self.target, 'OFFLINE_CWD')
                            self.assertNotIn(b'J5:', t.output)
                            self.assertNotIn(b'[y/N]', t.output)
                        self.assertFalse((self.state / 'cache/semantic-cache.db').exists())


if __name__ == '__main__':
    unittest.main()
