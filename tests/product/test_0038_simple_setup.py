"""Current no-quota interface and understandable directory privacy in real PTYs."""
import json
import unittest
import test_0032_ux as ux

class SimpleSetup(unittest.TestCase):
    setUp = ux.UXTasks.setUp
    tearDown = ux.UXTasks.tearDown
    cli = ux.UXTasks.cli
    config = ux.UXTasks.config
    process = ux.UXTasks.process

    def test_no_quota_setting_or_readiness_gate(self):
        self.cli('config', 'set', 'semantic', 'on')
        cfg = json.loads(self.config().read_text())
        self.assertNotIn('request_limit', cfg)
        self.assertEqual(cfg['schema_version'], 3)
        before = self.config().read_bytes()
        self.cli('config', 'set', 'request_limit', '10', code=2)
        self.assertEqual(before, self.config().read_bytes())
        self.assertNotIn(b'request_limit', self.cli('config', 'set', '--help').stdout)
        self.assertNotIn(b'allowance', self.cli('doctor').stdout)
        status = json.loads(self.cli('doctor', '--json').stdout)
        self.assertTrue(status['provider_configured'])

    def test_advanced_back_does_not_complete_or_save(self):
        with self.process('setup') as t:
            t.until(b'Semantic provider'); t.send('off\n')
            t.until(b'Local visit tracking'); t.send('\n')
            t.until(b'Advanced settings'); t.send('edit\n')
            t.until(b'Enter: done'); t.send('b\n')
            t.until(b'Advanced settings')
            self.assertFalse(self.config().exists())
            t.send('q\n'); t.cancelled()
            self.assertFalse(self.config().exists())

    def test_private_folder_explanation_and_actual_local_behavior(self):
        private = self.home / 'private-client'; private.mkdir()
        child = private / 'private-child'; child.mkdir()
        self.cli('record', '--', child, cwd=child)
        for lang in ('en', 'zh'):
            self.env['J_JUMP_LANG'] = lang
            self.cli('config', 'set', 'semantic', 'off')
            with self.process('setup') as t:
                t.until(('q Exit without saving' if lang == 'en' else 'q 退出不保存').encode()); t.send('3\n')
                done = ('Enter: done' if lang == 'en' else 'Enter 完成').encode()
                t.until(done); t.send('\n')
                t.until(done)
                self.assertIn(('names and paths are not sent to the selected provider' if lang == 'en' else '名称、路径不会发送给所选服务').encode(), t.output)
                self.assertIn(('subfolders' if lang == 'en' else '子文件夹').encode(), t.output)
                self.assertIn(('File contents are never uploaded' if lang == 'en' else '不会上传文件内容').encode(), t.output)
                self.assertIn(b'/work/', t.output)
                t.send(str(private) + '\n'); t.until(done); t.send('\n')
                t.until(('Candidate limit' if lang == 'en' else '候选数量').encode()); t.send('\n')
                t.until(('Language' if lang == 'en' else '显示语言').encode()); t.send('\n')
                t.until(('q Exit without saving' if lang == 'en' else 'q 退出不保存').encode())
                start = len(t.output)
                t.send('s\n'); t.until(('Saved.' if lang == 'en' else '已保存').encode())
                self.assertNotIn(str(private).encode(), t.output[start:])
                self.assertNotIn(b'Save this configuration', t.output)
            self.assertEqual(self.cli('query', 'private-child').stdout.strip(), str(child).encode())
            self.cli('preview', 'private-child', cwd=private, code=5)
