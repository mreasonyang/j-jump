import errno, json, os, pathlib, pty, select, shlex, shutil, signal, sqlite3, subprocess, tempfile, time, unittest
ROOT=pathlib.Path(__file__).resolve().parents[2]
BIN=pathlib.Path(os.environ.get('JJ_TEST_BIN',ROOT/'target/debug/jjump')).resolve()
class Terminal:
 def __init__(self,argv,env,cwd):
  self.pid,self.fd=pty.fork();self.output=b''
  if self.pid==0:
   os.chdir(cwd);os.execvpe(argv[0],argv,env)
 def send(self,text):os.write(self.fd,text.encode())
 def until(self,needle,timeout=5):
  deadline=time.monotonic()+timeout;buf=b''
  while needle not in buf and time.monotonic()<deadline:
   if select.select([self.fd],[],[],.05)[0]:
    try:data=os.read(self.fd,65536)
    except OSError as e:
     if e.errno==errno.EIO:break
     raise
    if not data:break
    buf+=data
  self.output+=buf
  if needle not in buf:raise AssertionError((needle,buf[-3000:]))
  return buf
 def cancelled(self,timeout=5):
  deadline=time.monotonic()+timeout
  while time.monotonic()<deadline:
   if select.select([self.fd],[],[],.02)[0]:
    try:self.output+=os.read(self.fd,65536)
    except OSError as e:
     if e.errno!=errno.EIO:raise
   pid,status=os.waitpid(self.pid,os.WNOHANG)
   if pid:
    self.pid=0
    if os.waitstatus_to_exitcode(status)!=130:raise AssertionError(('expected cancellation exit 130',status,self.output[-2000:]))
    if b'J130' in self.output:raise AssertionError('cancellation must be silent')
    return
  raise AssertionError(('cancellation did not finish',self.output[-2000:]))
 def close(self):
  if not self.pid:
   os.close(self.fd);return
  try:os.kill(self.pid,signal.SIGKILL)
  except ProcessLookupError:pass
  os.close(self.fd)
  deadline=time.monotonic()+5
  while True:
   try:reaped,_=os.waitpid(self.pid,os.WNOHANG)
   except ChildProcessError:break
   if reaped:break
   if time.monotonic()>deadline:raise AssertionError('test process did not exit after closed PTY and SIGKILL')
   time.sleep(.01)
class RealTerminal(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.root=pathlib.Path(self.tmp.name).resolve();self.home=self.root/'home';self.home.mkdir();self.a=self.home/'alpha space 中文';self.a.mkdir();self.b=self.home/'beta';self.b.mkdir();self.env=dict(os.environ,J_JUMP_PICKER="numbered",HOME=str(self.home),J_JUMP_HOME=str(self.root/'state'),PATH=str(BIN.parent)+':'+os.environ['PATH'],TERM='xterm',PS1='JJ> ',PS2='MORE> ',NO_COLOR='1');self.env.pop('TYPESAFE_API_KEY',None);self.env.pop('J_JUMP_CONFIG',None)
 def tearDown(self):self.tmp.cleanup()
 def cli(self,*args,cwd=None):return subprocess.run([str(BIN),*args],env=dict(self.env,PWD=str(cwd or self.home)),cwd=cwd or self.home,capture_output=True,check=True)
 def seeded(self):
  self.cli('record','--',str(self.a),cwd=self.a);self.cli('record','--',str(self.b),cwd=self.b)
 def test_picker_selection_cancellation_and_eof(self):
  self.seeded()
  # The picker order must not depend on whether the two fixture writes cross a second.
  with sqlite3.connect(self.root/'state/data/visits.db') as db:db.execute('update visits set last_seen=?',(int(time.time()),))
  for reply,expected in [('1\n',str(self.a).encode()),('\n',b'J130'),('\x04',b'J130')]:
   terminal=Terminal([str(BIN),'query','--interactive'],self.env,self.home)
   try:terminal.until(b'> ');terminal.send(reply);terminal.cancelled() if expected==b'J130' else terminal.until(expected)
   finally:terminal.close()
 def test_setup_cancel_preserves_configuration(self):
  t=Terminal([str(BIN),'setup'],self.env,self.home)
  try:t.until(b'Semantic provider');t.send('\x04');t.cancelled()
  finally:t.close()
  self.assertFalse((self.root/'state/config/config.json').exists())
 def test_setup_off_saved_atomically(self):
  t=Terminal([str(BIN),'setup'],self.env,self.home)
  try:
   t.until(b'Semantic provider');t.send('off\n');t.until(b'Local visit tracking');t.send('on\n');t.until(b'Advanced settings');t.send('\n');t.until(b'Saved.')
  finally:t.close()
  c=json.loads((self.root/'state/config/config.json').read_text());self.assertFalse(c['semantic'])
 def test_prompt_count_and_previous_each_shell(self):
  for shell in ['bash','zsh','fish']:
   with self.subTest(shell=shell):
    state=self.root/('state-'+shell);env=dict(self.env,J_JUMP_HOME=str(state),TERM='dumb' if shell=='fish' else 'xterm');init=self.root/('init-'+shell);init.write_bytes(self.cli('init',shell).stdout)
    argv={'bash':['bash','--noprofile','--norc','-i'],'zsh':['zsh','-f','-i'],'fish':['fish','--no-config','--interactive']}[shell]
    subprocess.run([str(BIN),'config','set','semantic','off'],env=env,capture_output=True,check=True)
    t=Terminal(argv,env,self.a)
    def step(command,token):
     t.send(command+"; printf '\\n"+token+"\\n'\n");t.until(('\r\n'+token+'\r\n').encode());time.sleep(.12)
    try:
     step('source '+shlex.quote(str(init)),'READY1')
     step('source '+shlex.quote(str(init)),'READY2')
     step('false','FALSE1')
     step('cd '+shlex.quote(str(self.b)),'CHANGED1')
     step('j -','PREVIOUS1')
     step("printf 'WHERE:%s\\n' \"$PWD\"",'LOCATED1')
     self.assertIn(('WHERE:'+str(self.a)).encode(),t.output)
     rows=dict(sqlite3.connect(state/'data/visits.db').execute('select path,count from visits'))
     self.assertEqual(rows,{str(self.a):2,str(self.b):1})
    finally:t.close()
 def test_zsh_current_directory_keyword_noop_without_jev_key(self):
  blog=self.home/'Blog';blog.mkdir();self.cli('record','--',str(blog),cwd=blog)
  self.cli('config','set','semantic','on')
  init=self.root/'noop-init-zsh';init.write_bytes(self.cli('init','zsh').stdout)
  t=Terminal(['zsh','-f','-i'],self.env,blog)
  try:
   t.send('source '+shlex.quote(str(init))+"; printf '\nNOOP-READY\n'\n");t.until(b'\r\nNOOP-READY\r\n');time.sleep(.12)
   for query,marker in [('bl','NOOP-BL'),('b','NOOP-B')]:
    t.send(f'j {query}; printf "{marker}:%s\\n" "$PWD"\n')
    output=t.until((marker+':'+str(blog)).encode())
    self.assertNotIn(b'J5:',output);self.assertNotIn(b'J3:',output)
  finally:t.close()
 def test_rapid_changed_prompts_count_before_another_command(self):
  for shell in ['bash','zsh','fish']:
   with self.subTest(shell=shell):
    state=self.root/('rapid-state-'+shell);env=dict(self.env,J_JUMP_HOME=str(state),TERM='dumb' if shell=='fish' else 'xterm');init=self.root/('rapid-init-'+shell);init.write_bytes(self.cli('init',shell).stdout)
    argv={'bash':['bash','--noprofile','--norc','-i'],'zsh':['zsh','-f','-i'],'fish':['fish','--no-config','--interactive']}[shell]
    t=Terminal(argv,env,self.a)
    try:
     t.send('source '+shlex.quote(str(init))+"; printf '\\nRAPID-READY\\n'\n");t.until(b'\r\nRAPID-READY\r\n');time.sleep(.12)
     t.send('cd '+shlex.quote(str(self.b))+'; cd '+shlex.quote(str(self.a))+'; cd '+shlex.quote(str(self.b))+"; printf '\\nBATCHED\\n'\n");t.until(b'\r\nBATCHED\r\n');time.sleep(.12)
     t.send('cd '+shlex.quote(str(self.a))+'\ncd '+shlex.quote(str(self.b))+"; printf '\\nRAPID-END\\n'\n");t.until(b'\r\nRAPID-END\r\n');time.sleep(.15)
     rows=dict(sqlite3.connect(state/'data/visits.db').execute('select path,count from visits'))
     self.assertEqual(rows,{str(self.a):2,str(self.b):2})
    finally:t.close()
 def test_live_supervisor_guard_is_bounded_and_retries_current_pwd(self):
  for shell in ['bash','zsh','fish']:
   with self.subTest(shell=shell):
    state=self.root/('guard-state-'+shell);env=dict(self.env,J_JUMP_HOME=str(state),TERM='dumb' if shell=='fish' else 'xterm');init=self.root/('guard-init-'+shell);init.write_bytes(self.cli('init',shell).stdout)
    argv={'bash':['bash','--noprofile','--norc','-i'],'zsh':['zsh','-f','-i'],'fish':['fish','--no-config','--interactive']}[shell]
    t=Terminal(argv,env,self.a);blocker=subprocess.Popen(['sleep','2'])
    try:
     t.send('source '+shlex.quote(str(init))+"; printf '\\nGUARD-READY\\n'\n");t.until(b'\r\nGUARD-READY\r\n');time.sleep(.12)
     assignment=('set -g __jj_record_pid '+str(blocker.pid)) if shell=='fish' else ('__jj_record_pid='+str(blocker.pid))
     t.send(assignment+"; printf '\\nGUARD-SET\\n'\n");t.until(b'\r\nGUARD-SET\r\n')
     started=time.monotonic();t.send('cd '+shlex.quote(str(self.b))+"; printf '\\nGUARD-BLOCKED\\n'\n");t.until(b'\r\nGUARD-BLOCKED\r\n')
     t.send("printf '\\nGUARD-AFTER\\n'\n");t.until(b'\r\nGUARD-AFTER\r\n')
     self.assertLess(time.monotonic()-started,.3)
     rows=dict(sqlite3.connect(state/'data/visits.db').execute('select path,count from visits'))
     self.assertEqual(rows,{str(self.a):1})
     blocker.terminate();blocker.wait(timeout=2)
     t.send("printf '\\nGUARD-RETRY\\n'\n");t.until(b'\r\nGUARD-RETRY\r\n');time.sleep(.15)
     rows=dict(sqlite3.connect(state/'data/visits.db').execute('select path,count from visits'))
     self.assertEqual(rows,{str(self.a):1,str(self.b):1})
    finally:
     if blocker.poll() is None:blocker.terminate();blocker.wait(timeout=2)
     t.close()
 def test_zsh_completion_changes_buffer_only(self):
  self.seeded();init=self.root/'init-z';init.write_bytes(self.cli('init','zsh').stdout)
  t=Terminal(['zsh','-f','-i'],self.env,self.home)
  try:
   t.send('source '+shlex.quote(str(init))+"; printf '\\nREADYZ\\n'\n");t.until(b'\r\nREADYZ\r\n');time.sleep(.1)
   t.send('j alpha \t');t.until(b'Choose a directory');t.send('1\r');t.until(b'j -- ');time.sleep(.15)
   t.send('\x03');time.sleep(.2);t.send("printf '\\nUNCHANGED:%s\\n' \"$PWD\"\n");t.until(('UNCHANGED:'+str(self.home)).encode())
  finally:t.close()
 def test_clear_during_picker_invalidates_selection(self):
  self.seeded();t=Terminal([str(BIN),'query','--interactive'],self.env,self.home)
  try:
   t.until(b'> ');self.cli('history','clear','--apply');t.send('1\n');t.until(b'J7:')
  finally:t.close()
 def test_forced_route_without_credential_stops_without_selection(self):
  # Failed semantics must not emit even an available local match.
  self.cli('record','--',str(self.a),cwd=self.a);self.cli('record','--',str(self.b),cwd=self.b)
  self.cli('config','set','semantic','on');self.cli('config','set','consent','always')
  t=Terminal([str(BIN),'--force-semantic','query','alph'],self.env,self.home)
  try:
   buf=t.until(b'J5:',timeout=20)
   self.assertIn(b'J5:',buf)
   self.assertNotIn(b'[Jev suggestion]',buf)
   self.assertNotIn(str(self.a).encode(),buf)
   self.assertNotIn(b'Choose a directory',buf)
  finally:t.close()
 def test_local_match_does_not_call_jev_without_force(self):
  self.cli('record','--',str(self.a),cwd=self.a)
  self.cli('config','set','semantic','on');self.cli('config','set','consent','always')
  t=Terminal([str(BIN),'query','alph'],self.env,self.home)
  try:
   buf=t.until(str(self.a).encode(),timeout=20)
   self.assertNotIn(b'J5:',buf)
  finally:t.close()
if __name__=='__main__':unittest.main()
