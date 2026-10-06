import errno,json,os,pathlib,pty,select,shlex,signal,subprocess,tempfile,time,unittest,fcntl,termios,struct,re
ROOT=pathlib.Path(__file__).resolve().parents[2]
BIN=pathlib.Path(os.environ.get('JJ_TEST_BIN',ROOT/'target/debug/jjump')).resolve()
class Terminal:
 def __init__(self,shell,env,cwd,dsr=True,term=None,*,startup_timeout=10):
  self.pid,self.fd=pty.fork();self.output=b'';self.dsr=dsr;self.shell_name=shell
  if self.pid==0:
   os.chdir(cwd);args={'bash':['bash','--noprofile','--norc','-i'],'zsh':['zsh','-f','-i'],'fish':['fish','--no-config','--interactive','--init-command',"function fish_prompt; printf 'JJ_TEST_READY# '; end"]}[shell];os.execvpe(args[0],args,dict(env,PS1='JJ_TEST_READY# ',TERM=term or ('dumb' if shell=='fish' else 'xterm')))
  try:
   fcntl.ioctl(self.fd,termios.TIOCSWINSZ,struct.pack('HHHH',30,160,0,0))
   # A fixed sleep confounds shell startup with the command being tested.
   # No test input is sent until the shell has rendered its first prompt.
   self.until(b'JJ_TEST_READY# ',startup_timeout)
  except BaseException as error:
   self.close()
   if isinstance(error,AssertionError):
    raise AssertionError(('shell startup timed out',shell,self.output[-3000:])) from error
   raise
 def send(self,s):os.write(self.fd,s.encode())
 def drain(self,seconds=.15):
  b=b'';end=time.monotonic()+seconds
  while time.monotonic()<end:
   if select.select([self.fd],[],[],.02)[0]:
    try:v=os.read(self.fd,65536)
    except OSError:break
    if not v:break
    b+=v
    if self.dsr and b'\x1b[5n' in v:self.send('\x1b[0n')
    if b'\x1b[6n' in v:self.send('\x1b[1;1R')
    if b'\x1b[0c' in v:self.send('\x1b[?1;2c')
    if b'\x1b[?2004$p' in v:self.send('\x1b[?2004;2$y')
  self.output+=b;return b
 def until(self,needle,seconds=4):
  b=b'';end=time.monotonic()+seconds
  while needle not in b and time.monotonic()<end:b+=self.drain(.05)
  if needle not in b:raise AssertionError(('terminal output timed out',needle,self.output[-3000:]))
  return b
 def cmd(self,s,marker):
  self.send(s+"; printf '\\n"+marker+"\\n'\r");self.until(('\r\n'+marker+'\r\n').encode());self.drain(.1)
 def send_and_wait_prompt(self,s,seconds=10):
  # A foreground command can still be restoring the editor after its last
  # output. Fish also redraws its prompt while accepting a command, so only a
  # complete prompt on a new line with terminal ownership restored is ready.
  start=len(self.output);self.send(s);end=time.monotonic()+seconds
  while True:
   tail=re.sub(rb'\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)',b'',self.output[start:])
   tail=re.sub(rb'\x1b\[[0-?]*[ -/]*[@-~]|\x1b[=>]',b'',tail)
   line=tail.rsplit(b'\r',1)[-1].rsplit(b'\n',1)[-1]
   if b'\n' in tail and line==b'JJ_TEST_READY# ' and os.tcgetpgrp(self.fd)==os.getpgid(self.pid):break
   if time.monotonic()>=end:raise AssertionError(('shell did not return to prompt',self.output[start:][-3000:]))
   self.drain(.05)
  return self.output[start:]
 def cancelled(self):
  start=len(self.output)
  variable='$status' if self.shell_name=='fish' else '$?'
  self.cmd('printf "CANCEL_STATUS:%s\\n" "'+variable+'"','CANCEL_DONE')
  if b'CANCEL_STATUS:130' not in self.output[start:]:raise AssertionError(('expected shell exit 130',self.output[start:]))
  if b'J130' in self.output[start:]:raise AssertionError('cancellation must be silent')
 def close(self):
  # Closing the PTY first releases a foreground editor's terminal I/O on macOS.
  # Signal only the child we own: the foreground group may already have exited.
  os.close(self.fd)
  try:os.kill(self.pid,signal.SIGKILL)
  except ProcessLookupError:pass
  deadline=time.monotonic()+5
  while True:
   try:reaped,_=os.waitpid(self.pid,os.WNOHANG)
   except ChildProcessError:break
   if reaped:break
   if time.monotonic()>deadline:raise AssertionError('test shell did not exit after closed PTY and SIGKILL')
   time.sleep(.01)

class NavigationFixes(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.root=pathlib.Path(self.tmp.name).resolve();self.home=self.root/'home';self.cwd=self.home/'neutral';self.cwd.mkdir(parents=True);self.target=self.home/'work'/'alpha $(touch PWNED) 双语 "quote"';self.target.mkdir(parents=True)
  self.env={'J_JUMP_PICKER':'numbered','HOME':str(self.home),'J_JUMP_HOME':str(self.root/'state'),'PATH':str(BIN.parent)+':'+os.environ['PATH'],'PWD':str(self.cwd),'LANG':'en_US.UTF-8','PS1':'FIX> '}
  self.cli('record','--',str(self.target),cwd=self.target)
 def tearDown(self):self.tmp.cleanup()
 def cli(self,*a,cwd=None):return subprocess.run([str(BIN),*a],env=dict(self.env,PWD=str(cwd or self.cwd)),cwd=cwd or self.cwd,capture_output=True,timeout=5)
 def shell(self,shell,dsr=True):
  # Isolate editor behavior from best-effort prompt writes; hook concurrency has separate tests.
  self.assertEqual(self.cli('config','set','tracking','off').returncode,0)
  f=self.root/(shell+'.sh');f.write_bytes(self.cli('init',shell).stdout);t=Terminal(shell,self.env,self.cwd,dsr)
  try:t.cmd('source '+shlex.quote(str(f)),'READY')
  except BaseException:
   t.close();raise
  return t
 def test_physical_enter_completion_and_execution_all_shells(self):
  for shell in ['bash','zsh','fish']:
   with self.subTest(shell=shell):
    t=self.shell(shell)
    try:
     if shell=='bash':t.cmd('bind \'"\\e[0n": redraw-current-line\'','BOUND')
     t.send('j alpha \t');t.until(b'> ');t.send('1\r');t.drain(.4)
     # Completion must have returned to line editor, with explicit safe path.
     self.assertIn(b'j -- ',t.output)
     self.assertFalse((self.cwd/'PWNED').exists())
     t.send('\r');t.drain(.25);t.cmd("printf '\\nWHERE:%s\\n' \"$PWD\"",'DONE')
     self.assertIn(('WHERE:'+str(self.target)).encode(),t.output)
     self.assertFalse((self.cwd/'PWNED').exists())
     if shell=='bash':
      start=len(t.output);t.cmd('bind -q redraw-current-line','RESTORED');self.assertRegex(t.output[start:],rb'\\(?:e|M-)\[0n')
      major=int(subprocess.check_output(['bash','-c','printf %s "${BASH_VERSINFO[0]}"'],env=self.env))
      if major>=4:
       t.cmd('bind -x \'"\\e[0n": printf RESTORE_ME\'','XBOUND')
       t.send('j alpha \t');t.until(b'> ');t.send('1\r');t.drain(.3);t.send('\r');t.drain(.2)
       start=len(t.output);t.cmd('bind -X','XRESTORED');self.assertIn(b'printf RESTORE_ME',t.output[start:])

    finally:t.close()
 def test_bash_unicode_query_completion(self):
  t=self.shell('bash')
  try:
   t.send('j 双语 \t');t.until(b'> ');t.send('1\r');t.drain(.3);self.assertIn(b'j -- ',t.output)
   t.send('\r');t.drain(.15);t.cmd("printf '\\nWHERE:%s\\n' \"$PWD\"",'UNICODE');self.assertIn(('WHERE:'+str(self.target)).encode(),t.output)
  finally:t.close()
 def test_bash_unicode_middle_of_line_does_not_open_picker(self):
  t=self.shell('bash')
  try:
   # Cursor after the first CJK character: byte offset5 equals full char length5.
   # Accepting either byte/character length would incorrectly pass this position.
   t.send('j 双语 \x1b[D\x1b[D\t');t.drain(.3)
   self.assertNotIn(b'Choose a directory',t.output)
   self.assertNotIn(b'j -- ',t.output)
   t.send('\x03');t.drain(.15)
   t.cmd("printf '\\nWHERE:%s\\n' \"$PWD\"",'MIDLINE')
   self.assertIn(('WHERE:'+str(self.cwd)).encode(),t.output)
  finally:t.close()
 def test_selection_only_edits_buffer_before_enter(self):
  for shell in ['bash','zsh','fish']:
   with self.subTest(shell=shell):
    t=self.shell(shell)
    try:
     t.send('j alpha \t');t.until(b'> ');t.send('1\r');t.drain(.3);self.assertIn(b'j -- ',t.output)
     t.send('\x03');t.drain(.15);t.cmd("printf '\\nSTAY:%s\\n' \"$PWD\"",'NOEXEC');self.assertIn(('STAY:'+str(self.cwd)).encode(),t.output)
    finally:t.close()
 def test_physical_cancel_preserves_buffer_and_cwd(self):
  for shell in ['bash','zsh','fish']:
   for cancel in ['\x1b','\x03']:
    with self.subTest(shell=shell,key=repr(cancel)):
     t=self.shell(shell)
     try:
      t.send('j alpha \t');t.until(b'> ');t.send(cancel);t.drain(.2);t.send('\x15');t.send("printf '\\nSTAY:%s\\n' \"$PWD\"\r");t.until(('STAY:'+str(self.cwd)).encode());self.assertNotIn(b'j -- ',t.output)
     finally:t.close()
 def test_bash_nonresponding_terminal_no_bad_completion(self):
  t=self.shell('bash',False)
  try:
   t.send('j alpha \t');t.until(b'> ');t.send('1\r');t.drain(.25);self.assertNotIn(str(self.target).encode()+b'/',t.output)
   t.send('\x03');t.drain(.1);t.cmd("printf '\\nSTAY:%s\\n' \"$PWD\"",'CANCELLED');self.assertIn(('STAY:'+str(self.cwd)).encode(),t.output)
  finally:t.close()
 def test_child_query_never_uses_direct_shortcut(self):
  self.assertEqual(self.cli('query','--','..','/').returncode,2)
  for q in ['.','..']:
   r=self.cli('query',q,'/');self.assertEqual(r.returncode,3);self.assertEqual(r.stdout,b'')
  parent=self.cwd/'alpha';child=parent/'leaf';child.mkdir(parents=True)
  self.cli('record','--',str(parent),cwd=parent)
  for _ in range(5):self.cli('record','--',str(child),cwd=child)
  # Ranking v2 prefers the exact basename even within the child-only scope.
  r=self.cli('query','alpha','/');self.assertEqual(r.returncode,0);self.assertEqual(r.stdout.decode().strip(),str(parent))
  self.assertEqual(self.cli('history','forget','--apply','--',str(parent)).returncode,0)
  # The physical alpha directory still exists, but direct-path lookup must not
  # bypass the child-only inventory query when it is no longer recorded.
  r=self.cli('query','alpha','/');self.assertEqual(r.returncode,0);self.assertEqual(r.stdout.decode().strip(),str(child))
  self.assertEqual(self.cli('query','..').stdout.decode().strip(),str(self.home))
 def test_child_interactive_fallback_cannot_escape(self):
  # The only recorded match is outside cwd; no descendant candidates means no picker.
  r=self.cli('query','--interactive','nonmatch','/');self.assertEqual(r.returncode,3);self.assertEqual(r.stdout,b'')
if __name__=='__main__':unittest.main()
