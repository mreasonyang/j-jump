"""Tev1 onboarding and edits through real shells; only synthetic loopback services."""
import fcntl
import itertools
import json
import os
import re
import select
import shlex
import socket
import signal
import time
import struct
import termios
import unittest

import test_0036_first_use as first
import test_tev1 as tev
from test_pty import Terminal as Process
from test_navigation_fixes import BIN


class Tev1Setup(unittest.TestCase):
    setUp = first.FirstUse.setUp
    profile = first.FirstUse.profile
    cli = tev.Tev1.cli
    shell = tev.Tev1.shell
    server = tev.Tev1.server
    configure = tev.Tev1.configure

    def label(self, en, zh):
        return (zh if self.env['J_JUMP_LANG'] == 'zh' else en).encode()

    def provider_fields(self, t, provider, url=None, model=None):
        t.until(self.label('Semantic provider [', '语义服务 [')); t.send(provider + '\r')
        t.until(self.label('Request permission' if provider == 'tev1' else 'Network permission',
                           '请求许可' if provider == 'tev1' else '联网许可')); t.send('\r')
        t.until(self.label('Fields [', '发送字段 [')); t.send('\r')
        if provider == 'tev1':
            t.until(self.label('Ollama URL [', 'Ollama 地址 [')); t.send((url or '') + '\r')
            t.until(self.label('Tev1 4B model [', 'Tev1 4B 模型 [')); t.send((model or '') + '\r')
            t.until(self.label('Local connection check [', '本机连接检查 [')); t.send('skip\r')
        else:
            if provider == 'clef-flash':
                t.until(self.label('Cloudflare Account ID [', 'Cloudflare Account ID [')); t.send('\r')
            t.until(self.label('API key [', 'API Key [')); t.send('skip\r')

    def test_bilingual_first_use_in_three_shells_resumes_j_and_ji(self):
        for shell, language, command in itertools.product(('bash','zsh','fish'), ('en','zh'), ('j','ji')):
            with self.subTest(shell=shell, language=language, command=command), self.server() as (url, requests):
                self.profile(shell+language+command); self.env['J_JUMP_LANG']=language
                self.env.pop('J_JUMP_OFFLINE', None)
                self.cli('record', '--', self.target, cwd=self.target)
                if command == 'ji':
                    # Prompt hooks can put the current directory ahead of the
                    # intended target. Exercise selection with that ordering.
                    for _ in range(3):
                        self.cli('record', '--', self.cwd, cwd=self.cwd)
                with self.shell(shell) as t:
                    fcntl.ioctl(t.fd, termios.TIOCSWINSZ, struct.pack('HHHH', 30, 60, 0, 0))
                    t.send(('j -- ' + shlex.quote(str(self.target)) if command == 'j' else 'ji') + '\r')
                    self.provider_fields(t, 'tev1', url)
                    t.until(self.label('Local visit tracking', '本地访问记录')); t.send('\r')
                    t.until(self.label('Advanced settings [', '高级设置 [')); t.send('\r')
                    output=t.until(self.label('Saved.', '已保存。'))
                    if command == 'ji':
                        picker=self.label('Empty/q: cancel', '空输入/q 取消')
                        if picker not in output: t.until(picker)
                        chosen = re.search(rb'(\d+)\. ~/' + re.escape(self.target.name.encode()), t.output)
                        self.assertIsNotNone(chosen, t.output)
                        self.assertNotEqual(chosen.group(1), b'1', t.output)
                        t.send(chosen.group(1).decode() + '\r')
                    t.drain(.2)
                    t.cmd('printf "WHERE:%s\\n" "$PWD"', 'DONE')
                    self.assertIn(('WHERE:'+str(self.target)).encode(),t.output)
                    self.assertIn(b'ollama pull tev1:4b-q8_0',t.output)
                    self.assertNotIn(b'API key [',t.output); self.assertNotIn(b'API Key [',t.output)
                self.assertEqual(requests,[])
                cfg=json.loads(self.config.read_text())
                self.assertEqual(cfg['providers']['tev1']['ollama_model'],'tev1:4b-q8_0')
                self.assertEqual(cfg['providers']['tev1']['ollama_url'],url)

    def test_all_provider_pairs_save_and_cancel_in_both_languages_and_three_shells(self):
        for shell, language, pair in itertools.product(('bash','zsh','fish'), ('en','zh'), itertools.permutations(('jev','clef-flash','tev1'),2)):
            source,target=pair
            with self.subTest(shell=shell,language=language,source=source,target=target), self.server() as (url,requests):
                self.profile(shell+language+source+target); self.configure(url)
                self.env['J_JUMP_LANG']=language
                self.env.pop('TYPESAFE_API_KEY',None); self.env.pop('CLOUDFLARE_AUTH_TOKEN',None)
                self.cli('config','set','ollama_model','tev1:4b-q4_K_M')
                self.cli('config','set','cloudflare_account_id','c'*32)
                self.cli('config','set','provider',source)
                before=self.config.read_bytes(); initial=json.loads(before)
                with self.shell(shell) as t:
                    for finish in ('q','s'):
                        t.send('jjump setup\r'); t.until(self.label('s Save; q Exit without saving','s 保存；q 退出不保存')); t.send('1\r')
                        self.provider_fields(t,target)
                        t.until(self.label('s Save; q Exit without saving','s 保存；q 退出不保存')); t.send(finish+'\r'); t.drain(.2)
                        t.cmd('true','FINISHED'+finish)
                        if finish=='q': self.assertEqual(self.config.read_bytes(),before)
                cfg=json.loads(self.config.read_text())
                self.assertEqual(cfg['provider'],target)
                self.assertEqual(cfg['providers']['tev1'],initial['providers']['tev1'])
                self.assertEqual(cfg['cloudflare_account_id'],'c'*32)
                self.assertFalse((self.state/'config/credential.epoch').exists()); self.assertEqual(requests,[])

    def start_local_edit(self, t):
        t.until(self.label('s Save; q Exit without saving','s 保存；q 退出不保存')); t.send('6\r')
        t.until(self.label('Ollama URL [','Ollama 地址 ['))

    def test_discovery_check_draft_binding_and_cancel(self):
        for language in ('en','zh'):
            with self.subTest(language=language), self.server() as (url,requests):
                self.profile(language); self.configure(url); self.env['J_JUMP_LANG']=language
                before=self.config.read_bytes()
                t=Process([str(BIN),'setup'],self.env,self.cwd)
                try:
                    self.start_local_edit(t); t.send('\r')
                    t.until(self.label('Tev1 4B model [','Tev1 4B 模型 [')); t.send('list\r')
                    t.until(self.label('Tev1 4B model [','Tev1 4B 模型 ['))
                    self.assertEqual(len(requests),2)
                    self.assertTrue(all(body is None for _,_,body in requests))
                    t.send('2\r') # sorted: 4b, q4_K_M, q8_0
                    t.until(self.label('Local connection check [','本机连接检查 [')); t.send('check\r')
                    t.until(self.label('s Save; q Exit without saving','s 保存；q 退出不保存'))
                    self.assertEqual(requests[-1][2]['model'],'tev1:4b-q4_K_M')
                    self.assertEqual(self.config.read_bytes(),before)
                    t.send('7\r'); t.until(self.label('s Save; q Exit without saving','s 保存；q 退出不保存'))
                    self.assertIn(self.label('passed for these draft settings','本次会话已按当前草稿设置检查通过'),t.output)
                    # Changing the model invalidates the session readiness claim.
                    t.send('6\r'); t.until(self.label('Ollama URL [','Ollama 地址 [')); t.send('\r')
                    t.until(self.label('Tev1 4B model [','Tev1 4B 模型 [')); t.send('tev1:4b-q8_0\r')
                    t.until(self.label('Local connection check [','本机连接检查 [')); t.send('skip\r')
                    t.until(self.label('s Save; q Exit without saving','s 保存；q 退出不保存')); start=len(t.output)
                    t.send('7\r'); t.until(self.label('s Save; q Exit without saving','s 保存；q 退出不保存'))
                    self.assertIn(self.label('not checked','尚未检查'),t.output[start:])
                    t.send('q\r'); t.cancelled()
                finally: t.close()
                self.assertEqual(self.config.read_bytes(),before)
                self.assertEqual(len(requests),5)

    def test_offline_discovery_and_check_never_contact_service(self):
        for flag in (False,True):
            with self.subTest(flag=flag), self.server() as (url,requests):
                self.profile(str(flag)); self.configure(url)
                if not flag: self.env['J_JUMP_OFFLINE']='1'
                before=self.config.read_bytes()
                t=Process([str(BIN),*(['--offline'] if flag else []),'setup'],self.env,self.cwd)
                try:
                    self.start_local_edit(t); t.send('\r'); t.until(b'Tev1 4B model ['); t.send('list\r')
                    t.until(b'Tev1 4B model ['); self.assertIn(b'Offline: enter a model',t.output); t.send('\r')
                    t.until(b'Local connection check ['); t.send('check\r')
                    t.until(b'Local connection check ['); self.assertIn(b'Cannot check in offline mode',t.output)
                    t.send('help\r'); t.until(b'Local connection check ['); t.send('skip\r')
                    t.until(b's Save; q Exit without saving'); t.send('q\r'); t.cancelled()
                finally: t.close()
                self.assertEqual(requests,[]); self.assertEqual(self.config.read_bytes(),before)

    def test_fail_recover_retry_back_invalid_input_and_save(self):
        with self.server() as (url,requests):
            self.configure(url)
            with socket.socket() as sock:
                sock.bind(('127.0.0.1',0)); unavailable='http://127.0.0.1:'+str(sock.getsockname()[1])
            t=Process([str(BIN),'setup'],self.env,self.cwd)
            try:
                self.start_local_edit(t); t.send('https://remote.invalid\r')
                t.until(b'Ollama URL ['); self.assertIn(b'ollama_url expects',t.output); t.send(unavailable+'\r')
                t.until(b'Tev1 4B model ['); t.send('tev1:4b-mlx\r')
                t.until(b'Tev1 4B model ['); self.assertIn(b'ollama_model expects',t.output); t.send('\r')
                t.until(b'Local connection check ['); t.send('check\r')
                t.until(b'Local connection check ['); self.assertIn(b'Cannot connect',t.output); self.assertIn(unavailable.encode(),t.output)
                t.send('b\r'); t.until(b'Tev1 4B model ['); t.send('b\r'); t.until(b'Ollama URL ['); t.send(url+'\r')
                t.until(b'Tev1 4B model ['); t.send('\r'); t.until(b'Local connection check ['); t.send('check\r')
                t.until(b's Save; q Exit without saving'); self.assertIn(b'Local connection check passed',t.output)
                t.send('s\r'); t.until(b'Saved.')
            finally: t.close()
            self.assertEqual(json.loads(self.config.read_text())['providers']['tev1']['ollama_url'],url)
            self.assertEqual(len(requests),3)

    def test_cancel_eof_interrupt_and_save_conflict_keep_saved_config(self):
        for cancel in ('q\r','\x04','\x03','conflict'):
            with self.subTest(cancel=cancel), self.server() as (url,requests):
                self.profile(str(ord(cancel[0]))); self.configure(url); before=self.config.read_bytes()
                t=Process([str(BIN),'setup'],self.env,self.cwd)
                try:
                    self.start_local_edit(t); t.send('\r'); t.until(b'Tev1 4B model [')
                    if cancel=='conflict':
                        t.send('tev1:4b-q4_K_M\r'); t.until(b'Local connection check ['); t.send('skip\r')
                        t.until(b's Save; q Exit without saving')
                        self.cli('config','set','ollama_model','tev1:4b-bf16'); before=self.config.read_bytes()
                        t.send('s\r'); t.until(b'configuration changed concurrently')
                    else:
                        t.send(cancel)
                        if cancel == '\x03':
                            deadline=time.monotonic()+5
                            while True:
                                if select.select([t.fd],[],[],.02)[0]:
                                    try: t.output += os.read(t.fd,65536)
                                    except OSError: pass
                                pid,status=os.waitpid(t.pid,os.WNOHANG)
                                if pid:
                                    t.pid=0
                                    self.assertIn(os.waitstatus_to_exitcode(status),(130,-signal.SIGINT))
                                    break
                                self.assertLess(time.monotonic(),deadline,'interrupt did not finish')
                                time.sleep(.02)
                        else: t.cancelled()
                finally: t.close()
                self.assertEqual(self.config.read_bytes(),before); self.assertEqual(requests,[])

    def test_interrupt_during_check_returns_to_each_shell_without_saving(self):
        for shell in ('bash','zsh','fish'):
            with self.subTest(shell=shell), self.server('timeout') as (url,requests):
                self.profile('check-interrupt-'+shell); self.configure(url); before=self.config.read_bytes()
                with self.shell(shell) as t:
                    t.send('jjump setup\r'); self.start_local_edit(t); t.send('\r')
                    t.until(b'Tev1 4B model ['); t.send('\r'); t.until(b'Local connection check ['); t.send('check\r')
                    deadline=time.monotonic()+5
                    while len(requests)<3:
                        t.drain(.02)
                        self.assertLess(time.monotonic(),deadline)
                    started=time.monotonic(); t.send('\x03'); t.drain(.2)
                    t.cmd('printf "RETAINED:%s\\n" "$PWD"','INTERRUPTED')
                    self.assertLess(time.monotonic()-started,2)
                    self.assertIn(('RETAINED:'+str(self.cwd)).encode(),t.output)
                self.assertEqual(self.config.read_bytes(),before)

    def test_metadata_only_cli_and_empty_incompatible_list(self):
        for outcome in ('ok','missing','mlx','old'):
            with self.subTest(outcome=outcome), self.server(outcome) as (url,requests):
                self.profile(outcome); self.configure(url)
                r=self.cli('provider-check','--models','--json',code=5 if outcome=='old' else 0)
                if outcome!='old':
                    report=json.loads(r.stdout)
                    self.assertEqual(len(report['models']),3 if outcome=='ok' else 0)
                    self.assertEqual(report['synthetic_request'],'not run')
                self.assertFalse(any(body for _,_,body in requests))
                self.assertTrue(all('Authorization' not in headers for _,headers,_ in requests))
                before=len(requests); self.cli('--offline','provider-check','--models',code=2); self.assertEqual(len(requests),before)

if __name__=='__main__': unittest.main()
