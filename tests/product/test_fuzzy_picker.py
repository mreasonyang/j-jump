"""Real optional fzf acceptance; isolated synthetic paths and hostile option fixtures."""
import os,pathlib,shlex,shutil,subprocess,unittest,fcntl,termios,struct,time
import test_navigation_fixes as navigation
Terminal=navigation.Terminal
BIN=navigation.BIN

class FuzzyPicker(navigation.NavigationFixes):
 # Inherit fixtures/helpers, not the numbered-mode tests.
 def setUp(self):
  super().setUp();self.env['J_JUMP_PICKER']='fzf';self.env['TERM']='xterm'
  self.fzf=shutil.which('fzf')
  if not self.fzf:self.skipTest('real fzf unavailable; optional adapter runtime not verified')
  self.cli('config','set','tracking','off')
 def shell(self,shell,dsr=True):
  f=self.root/(shell+'.sh');f.write_bytes(self.cli('init',shell).stdout)
  t=Terminal(shell,self.env,self.cwd,dsr,term='xterm');t.cmd('source '+shlex.quote(str(f)),'READY');return t
 def choose(self,t,command,query):
  t.send(command);t.until(b'Directory>');t.drain(.15);t.send(query);t.drain(.15)
 def test_fuzzy_completion_and_ji_all_shells(self):
  # Numbered control tests deliberately use plain mode; this exercises real fzf.
  for shell in ['bash','zsh','fish']:
   for completion in [False,True]:
    with self.subTest(shell=shell,completion=completion):
     t=self.shell(shell)
     try:
      self.choose(t,'j alpha \t' if completion else 'ji\r','双语')
      t.send('\r');t.drain(.3)
      if completion:
       self.assertIn(b'j -- ',t.output);t.send('\r');t.drain(.2)
      t.cmd("printf '\\nWHERE:%s\\n' \"$PWD\"",'CHOSEN')
      self.assertIn(('WHERE:'+str(self.target)).encode(),t.output)
      self.assertFalse((self.cwd/'PWNED').exists())
     finally:t.close()
 def test_fuzzy_cancel_and_completion_no_auto_execute(self):
  for shell in ['bash','zsh','fish']:
   for key in ['\x1b','\x03','\x04']:
    t=self.shell(shell)
    try:
     self.choose(t,'j alpha \t','双语');t.send(key);t.drain(.25)
     t.send('\x15');t.cmd("printf '\\nSTAY:%s\\n' \"$PWD\"",'CANCEL')
     self.assertIn(('STAY:'+str(self.cwd)).encode(),t.output)
     self.assertNotIn(b'j -- ',t.output)
    finally:t.close()
   t=self.shell(shell)
   try:
    self.choose(t,'j alpha \t','双语');t.send('\r');t.drain(.2);t.send('\x03');t.drain(.2)
    t.cmd("printf '\\nSTAY:%s\\n' \"$PWD\"",'NOEXEC');self.assertIn(('STAY:'+str(self.cwd)).encode(),t.output)
   finally:t.close()
 def test_fuzzy_arrows_pages_empty_results_and_backspace(self):
  self.cli('config','set','tracking','on')
  for i in range(45):
   p=self.home/'large'/(f'forest-{i:03d}-leaf');p.mkdir(parents=True);self.cli('record','--',str(p),cwd=p)
  self.cli('config','set','tracking','off')
  t=self.shell('zsh')
  try:
   fcntl.ioctl(t.fd,termios.TIOCSWINSZ,struct.pack('HHHH',12,60,0,0))
   self.choose(t,'ji\r','zzzznomatch');t.drain(.2)
   self.assertNotIn(b'J130',t.output) # Empty Enter stays in filter, no jump.
   t.send('\x15');t.send('frst');t.drain(.2) # Genuine omitted-character match.
   t.send('\x1b[6~');t.drain(.1);t.send('\x1b[5~');t.send('\x1b[B');t.send('\x1b[A')
   t.send('\x15');t.send('frst044X');t.send('\x7f');t.drain(.2);t.send('\r');t.drain(.2)
   t.cmd("printf '\\nWHERE:%s\\n' \"$PWD\"",'PAGE')
   self.assertIn(('WHERE:'+str(self.home/'large'/'forest-044-leaf')).encode(),t.output)
  finally:t.close()
 def test_fzf_options_and_credentials_are_not_inherited(self):
  marker=self.root/'EXECUTED';opts=self.root/'options';opts.write_text('--bind=start:execute(touch '+str(marker)+')\n')
  self.env.update(FZF_DEFAULT_OPTS='--bind=start:execute(touch '+str(marker)+')',FZF_DEFAULT_OPTS_FILE=str(opts),FZF_DEFAULT_COMMAND='touch '+str(marker),TYPESAFE_API_KEY='synthetic-do-not-inherit')
  t=self.shell('bash')
  try:
   self.choose(t,'ji\r','双语');t.send('\r');t.drain(.2);t.cmd('true','SAFE');self.assertFalse(marker.exists())
  finally:t.close()
  # Inspect the actual child env using a fixture, with otherwise identical adapter path.
  fake=self.root/'fake';fake.mkdir();exe=fake/'fzf'
  exe.write_text('#!/usr/bin/python3\nimport os,sys\nassert not any(k.startswith("FZF_") or k=="TYPESAFE_API_KEY" for k in os.environ)\nsys.stdin.buffer.read()\nsys.exit(130)\n');exe.chmod(0o755);self.env['PATH']=str(fake)+':'+self.env['PATH']
  t=self.shell('bash')
  try:t.send('ji\r');t.cancelled();self.assertNotIn(b'AssertionError',t.output)
  finally:t.close()
 def test_fuzzy_snapshot_clear_rejected(self):
  t=self.shell('zsh')
  try:
   self.choose(t,'ji\r','双语');self.cli('history','clear','--apply');t.send('\r');t.until(b'J7:');t.cmd("printf '\\nSTAY:%s\\n' \"$PWD\"",'STALE');self.assertIn(('STAY:'+str(self.cwd)).encode(),t.output)
  finally:t.close()
 def test_missing_and_failed_fzf_never_switch_mode(self):
  fake=self.root/'fake';fake.mkdir();exe=fake/'fzf';self.env['PATH']=str(fake)+':'+self.env['PATH']
  for body,result in [('exit 2','J5'),('printf "0\\tforged\\0"','J130'),('exit 130','J130')]:
   exe.write_text('#!/bin/sh\ncat >/dev/null\n'+body+'\n');exe.chmod(0o755)
   t=self.shell('bash')
   try:
    t.send('ji\r')
    t.cancelled() if result=="J130" else t.until(result.encode());self.assertNotIn(b'Choose a directory',t.output)
   finally:t.close()
  missing=self.root/'no-fzf';missing.mkdir()
  for name,source in [('bash',shutil.which('bash')),('jjump',BIN),('env',shutil.which('env'))]:(missing/name).symlink_to(source)
  self.env['PATH']=str(missing)
  t=self.shell('bash')
  try:
   t.send('ji\r');t.until(b'J5');self.assertNotIn(b'Choose a directory',t.output)
   t.cmd("printf '\\nWHERE:%s\\n' \"$PWD\"",'ABSENT');self.assertIn(('WHERE:'+str(self.cwd)).encode(),t.output)
  finally:t.close()
  r=self.cli('query','--interactive');self.assertEqual(r.returncode,4)

# Existing inherited test methods exercise numbered input and belong to their own suite.
for _name in list(navigation.NavigationFixes.__dict__):
 if _name.startswith('test_'):setattr(FuzzyPicker,_name,None)
if __name__=='__main__':unittest.main()
