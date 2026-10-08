"""OpenAI setup and shell selection using synthetic keys and local IPC replies."""
import json
import termios
import unittest

import test_cloudflare as cloud
import test_0033_current_contract as current

SECRET = 'synthetic-openai-key'


class OpenAI(unittest.TestCase):
    setUp = cloud.Cloudflare.setUp
    profile = cloud.Cloudflare.profile
    cli = cloud.Cloudflare.cli
    process = cloud.Cloudflare.process
    finish = cloud.Cloudflare.finish

    def test_config_preview_and_offline_navigation_need_no_key(self):
        self.cli('config', 'set', 'provider', 'openai')
        self.cli('config', 'set', 'semantic', 'on')
        self.env['TYPESAFE_API_KEY'] = 'synthetic-jev-key'
        self.env['CLOUDFLARE_AUTH_TOKEN'] = 'synthetic-cloudflare-token'
        status = json.loads(self.cli('credential', 'status').stdout)
        self.assertEqual(status['provider'], 'openai')
        self.assertFalse(status['environment_present'])
        self.cli('record', '--', self.target, cwd=self.target)
        self.assertEqual(self.cli('query', 'alpha').stdout.strip(), str(self.target).encode())
        preview = json.loads(self.cli('preview', 'backend').stdout)
        self.assertEqual(preview['model'], 'gpt-6-luna')
        self.assertEqual(preview['questions'][0]['name'], 'destination')
        self.assertIsInstance(preview['input'], str)
        self.assertNotIn(str(self.home), json.dumps(preview))
        self.assertFalse((self.state / 'cache/semantic-drivers.db').exists())

    def test_doctor_checks_selected_environment_without_network(self):
        self.env.pop('J_JUMP_OFFLINE')
        self.cli('config', 'set', 'provider', 'openai')
        self.cli('config', 'set', 'semantic', 'on')
        self.assertFalse(json.loads(self.cli('doctor', '--json').stdout)['environment_ready'])
        self.assertIn(b'OpenAI blocked: no credential configured.', self.cli('doctor').stdout)
        self.env['OPENAI_API_KEY'] = SECRET
        self.assertTrue(json.loads(self.cli('credential', 'status').stdout)['environment_present'])
        status = json.loads(self.cli('doctor', '--json').stdout)
        self.assertTrue(status['environment_ready'])
        self.assertEqual(status['provider_test'], 'not run')
        for output in (self.cli('doctor').stdout, self.cli('config', 'show', '--json').stdout):
            self.assertNotIn(SECRET.encode(), output)
        self.env['OPENAI_API_KEY'] = 'invalid\nkey'
        self.assertFalse(json.loads(self.cli('doctor', '--json').stdout)['environment_ready'])

    def test_hidden_key_saves_to_openai_namespace(self):
        with self.process(fixture=True) as t:
            cloud.Cloudflare.choose(self, t, 'openai')
            t.send('enter\r'); t.until(b'OpenAI API key (hidden;')
            self.assertFalse(termios.tcgetattr(t.fd)[3] & termios.ECHO)
            t.send(SECRET + '\r'); self.finish(t)
            self.assertIn(b'FIXTURE: credential stored in memory', t.output)
            self.assertNotIn(SECRET.encode(), t.output)
        cfg = json.loads(self.config.read_text())
        self.assertEqual(cfg['provider'], 'openai')
        self.assertEqual(cfg['providers']['openai']['credential'], 'system')
        self.assertEqual(cfg['credential'], 'environment')
        self.assertEqual(cfg['cloudflare_credential'], 'environment')
        self.assertNotIn(SECRET, self.config.read_text())
        self.cli('config', 'set', 'provider', 'jev')
        self.cli('config', 'set', 'provider', 'openai')
        status = json.loads(self.cli('credential', 'status').stdout)
        self.assertEqual(status['configured_source'], 'system')

    def test_environment_setup_and_cancel_preserve_state(self):
        with self.process(extra={'OPENAI_API_KEY': SECRET}) as t:
            cloud.Cloudflare.choose(self, t, 'openai')
            t.send('environment\r'); self.finish(t)
            self.assertNotIn(SECRET.encode(), t.output)
        self.assertEqual(json.loads(self.config.read_text())['providers']['openai']['credential'], 'environment')
        before = self.config.read_bytes()
        # Existing setup menu: semantic settings, then cancel at the key input.
        with self.process(fixture=True) as t:
            t.until(b'1 Semantic provider'); t.send('1\r')
            cloud.Cloudflare.choose(self, t, 'openai')
            t.send('enter\r'); t.until(b'OpenAI API key (hidden;')
            t.send(SECRET + '\x03'); t.cancelled()
            self.assertNotIn(b'FIXTURE: credential stored in memory', t.output)
            self.assertNotIn(SECRET.encode(), t.output)
        self.assertEqual(self.config.read_bytes(), before)


class OpenAISelection(unittest.TestCase):
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
    assert_dispatch = current.CurrentContract.assert_dispatch

    def configure(self, suffix):
        current.CurrentContract.configure_semantics(self, suffix)
        self.env['OPENAI_API_KEY'] = SECRET
        self.cli('config', 'set', 'provider', 'openai')

    def test_suggestion_requires_explicit_selection_in_three_shells(self):
        for shell in ('bash', 'zsh', 'fish'):
            self.configure('openai-' + shell)
            with self.subTest(shell=shell), self.shell(shell) as t, self.adapter_reply('valid') as frames:
                t.send('ji notlexical\r'); t.until(b'Empty/q: cancel')
                self.assertIn(b'[OpenAI]', t.output)
                self.assertIn(b'OpenAI suggestion:', t.output)
                t.send_and_wait_prompt('1\r')
                t.cmd('printf "WHERE:%s\\n" "$PWD"', 'SELECTED')
                self.assertIn(('WHERE:' + str(self.target)).encode(), t.output)
                self.assertNotIn(SECRET.encode(), t.output)
                self.assertEqual(frames, ['Send']); self.assert_dispatch()

    def test_refusal_abstention_and_errors_do_not_move_or_open_picker(self):
        for outcome in ('refusal', 'none', 'invalid', 'error', 'binding', 'rate_limit'):
            self.configure('openai-' + outcome)
            with self.subTest(outcome=outcome), self.process('query', '--interactive', 'notlexical') as t, self.adapter_reply(outcome) as frames:
                t.until(b'J3:' if outcome in ('refusal', 'none') else b'J5:')
                self.assertNotIn(b'Choose a directory', t.output)
                self.assertNotIn(SECRET.encode(), t.output)
                self.assertEqual(frames, ['Send']); self.assert_dispatch()


if __name__ == '__main__':
    unittest.main()
