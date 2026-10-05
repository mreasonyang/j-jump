import json, os, pathlib, shutil, sqlite3, subprocess, tempfile, unittest
ROOT=pathlib.Path(__file__).resolve().parents[2]
BIN=pathlib.Path(os.environ.get('JJ_TEST_BIN',ROOT/'target/debug/jjump')).resolve()
class ProductCLI(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.root=pathlib.Path(self.tmp.name).resolve();self.home=self.root/'home';self.home.mkdir();self.env=dict(os.environ,J_JUMP_PICKER="numbered",HOME=str(self.home),J_JUMP_HOME=str(self.root/'state'),PATH=str(BIN.parent)+':'+os.environ['PATH']);self.env.pop('TYPESAFE_API_KEY',None);self.env.pop('J_JUMP_CONFIG',None);self.env.pop('J_JUMP_OFFLINE',None)
  self.a=self.home/'alpha space 中文';self.a.mkdir();self.b=self.home/'beta';self.b.mkdir()
 def tearDown(self):self.tmp.cleanup()
 def cli(self,*args,cwd=None,code=0):
  env=dict(self.env,PWD=str(cwd or self.home));r=subprocess.run([str(BIN),*map(str,args)],cwd=cwd or self.home,env=env,capture_output=True,timeout=5)
  self.assertEqual(r.returncode,code,(args,r.stdout,r.stderr));return r
 def record(self,p):self.cli('record','--',p,cwd=p)
 def rows(self):return sqlite3.connect(self.root/'state/data/visits.db').execute('select path,count from visits').fetchall()
 def test_direct_paths_without_valid_config(self):
  self.cli('config','set','semantic','off');(self.root/'state/config/config.json').write_text('broken')
  self.assertEqual(self.cli('query','--',self.a).stdout,(str(self.a)+'\n').encode());self.cli('query','unknown',code=7)
 def test_local_keywords_and_explicit_failure(self):
  self.record(self.a);self.assertEqual(self.cli('query','alpha').stdout,(str(self.a)+'\n').encode());self.assertEqual(self.cli('query','./missing',code=6).stdout,b'');self.cli('query','unknown',code=3)
 def test_recorded_current_directory_is_local_noop(self):
  blog=self.home/'Blog';blog.mkdir();self.record(blog)
  for query in ('b','bl'):
   result=self.cli('query',query,cwd=blog)
   self.assertEqual(result.stdout,(str(blog)+'\n').encode());self.assertEqual(result.stderr,b'')
  self.assertEqual(self.cli('query','bl').stdout,(str(blog)+'\n').encode())
  other=self.home/'Build';other.mkdir();self.record(other)
  self.assertEqual(self.cli('query','b',cwd=blog).stdout,(str(other)+'\n').encode())
  self.cli('query','b','/',cwd=blog,code=3)
  self.cli('config','set','exclude',json.dumps([str(blog)]))
  self.cli('query','bl',cwd=blog,code=3)
  self.cli('query','--interactive','bl',cwd=blog,code=3)
 def test_record_requires_actual_cwd(self):self.cli('record','--',self.a,code=6)
 def test_counts_and_pause(self):
  self.record(self.a);self.record(self.a);self.assertEqual(self.rows(),[(str(self.a),2)]);self.cli('config','set','tracking','off');self.record(self.a);self.assertEqual(self.rows(),[(str(self.a),2)])
 def test_history_preview_apply(self):
  self.record(self.a);self.cli('history','clear','--preview');self.assertEqual(len(self.rows()),1);self.cli('history','clear','--apply');self.assertEqual(self.rows(),[])
 def test_scoped_forget(self):
  self.record(self.a);self.record(self.b);self.cli('history','forget','--apply','--',self.a);self.assertEqual(self.rows(),[(str(self.b),1)])
 def test_sensitive_and_symlink_privacy(self):
  ssh=self.home/'.ssh';ssh.mkdir();link=self.home/'linked';link.symlink_to(ssh);self.record(ssh);self.record(link);self.assertEqual(self.rows(),[])
 def test_exclusion_immediate(self):
  self.record(self.a);self.cli('config','set','exclude',json.dumps([str(self.a)]));self.cli('query','alpha',code=3);self.record(self.a);self.assertEqual(self.rows(),[(str(self.a),1)])
 def test_no_tty_picker_never_reads_pipe(self):
  self.record(self.a);r=self.cli('query','--interactive',code=4);self.assertEqual(r.stdout,b'')
 def test_noninteractive_setup(self):self.cli('setup',code=4)
 def test_doctor_redacted(self):
  r=self.cli('doctor','--json');v=json.loads(r.stdout);self.assertEqual(v['provider_test'],'not run');self.assertNotIn(str(self.home).encode(),r.stdout)
 def test_query_injection_is_data(self):
  name='$(touch PWNED); `whoami`';p=self.home/name;p.mkdir();self.record(p);self.assertEqual(self.cli('query','--',p).stdout,(str(p)+'\n').encode());self.assertFalse((self.home/'PWNED').exists())
 def test_leading_dash_literal_path(self):
  p=self.home/'-';p.mkdir();self.assertEqual(self.cli('query','--','-',cwd=self.home).stdout,(str(p)+'\n').encode())
 def test_empty_query_rejected(self):self.cli('query','',code=2)
 def test_invalid_bytes_no_stdout(self):
  p=self.home/'new\nline';p.mkdir();self.assertEqual(self.cli('query','--',p,code=6).stdout,b'')
 def test_unknown_config_no_plaintext_fallback(self):self.cli('config','set','api_key','fake',code=2)
 def test_old_proxy_value_is_rejected_without_conversion(self):
  self.cli('config','set','semantic','off')
  path=self.root/'state/config/config.json';data=json.loads(path.read_text());data['proxy']='none';path.write_text(json.dumps(data));before=path.read_bytes()
  self.cli('config','show','--json',code=7);self.cli('config','set','tracking','off',code=7);self.assertEqual(path.read_bytes(),before)
 def test_reset_retains_privacy(self):
  self.cli('config','set','tracking','off');self.cli('config','set','exclude',json.dumps([str(self.a)]));self.cli('config','reset','--apply');v=json.loads(self.cli('config','show','--json').stdout);self.assertFalse(v['saved']['tracking']);self.assertEqual(v['saved']['exclude'],[str(self.a)])
 def test_corrupt_store_preserved(self):
  self.record(self.a);p=self.root/'state/data/visits.db';p.write_bytes(b'corrupt');self.cli('query','alpha',code=7);self.assertEqual(p.read_bytes(),b'corrupt');self.cli('query','--',self.a)
 def test_stale_lexical_match_not_semantic_miss(self):
  self.record(self.a);self.a.rmdir();self.cli('query','alpha',code=6)
 def test_preview_no_remote_and_no_absolute_paths(self):
  self.record(self.a);r=self.cli('preview','alpha');v=json.loads(r.stdout);self.assertEqual(v['model'],'jev-1.13.0');self.assertNotIn(str(self.home).encode(),r.stdout)
 def test_bash_zsh_native_previous_and_repeat_source(self):
  for shell in ['bash','zsh']:
   with self.subTest(shell=shell):
    init=self.cli('init',shell).stdout.decode();file=self.root/(shell+'.init');file.write_text(init)
    script='source "$1"; source "$1"; j -- "$2" || exit; builtin cd -- "$3"; j - >/dev/null; printf "RESULT:%s\\n" "$PWD"'
    r=subprocess.run([shell,'--noprofile','--norc','-c',script,'test',str(file),str(self.a),str(self.b)] if shell=='bash' else [shell,'-f','-c',script,'test',str(file),str(self.a),str(self.b)],env=self.env,cwd=self.home,capture_output=True)
    self.assertEqual(r.returncode,0,r.stderr);self.assertIn(('RESULT:'+str(self.a)).encode(),r.stdout)
 def test_bash_zsh_conflict_is_atomic(self):
  for shell in ['bash','zsh']:
   file=self.root/(shell+'.init');file.write_bytes(self.cli('init',shell).stdout)
   script='ji() { :; }; source "$1"; rc=$?; type j >/dev/null 2>&1 && exit 99; exit "$rc"'
   r=subprocess.run([shell,'-c',script,'test',str(file)],env=self.env,cwd=self.home,capture_output=True)
   self.assertEqual(r.returncode,2,(shell,r.stdout,r.stderr))
 def test_fish_native_previous(self):
  fish=shutil.which('fish');self.assertTrue(fish);file=self.root/'init.fish';file.write_bytes(self.cli('init','fish').stdout)
  script='source $argv[1]; source $argv[1]; j -- $argv[2]; or exit; cd -- $argv[3]; j - >/dev/null; printf "RESULT:%s\\n" $PWD'
  r=subprocess.run([fish,'--no-config','-c',script,str(file),str(self.a),str(self.b)],env=self.env,cwd=self.home,capture_output=True)
  self.assertEqual(r.returncode,0,r.stderr);self.assertIn(('RESULT:'+str(self.a)).encode(),r.stdout)
 def test_backup_restore_epoch_and_atomic_failure(self):
  self.record(self.a);backup=self.root/'backup'/'visits.json';self.cli('history','backup',backup)
  db=sqlite3.connect(self.root/'state/data/visits.db');old=db.execute('select epoch from meta').fetchone()[0];db.close()
  self.cli('history','clear','--apply');self.cli('history','restore',backup);self.assertEqual(self.rows(),[]);self.cli('history','restore','--apply',backup);self.assertEqual(self.rows(),[(str(self.a),1)])
  db=sqlite3.connect(self.root/'state/data/visits.db');self.assertNotEqual(db.execute('select epoch from meta').fetchone()[0],old);db.close()
  backup.write_text('{"schema_version":99,"records":[]}');self.cli('history','restore','--apply',backup,code=7);self.assertEqual(self.rows(),[(str(self.a),1)])
 def test_concurrent_visits_no_lost_successful_updates(self):
  self.record(self.a);env=dict(self.env,PWD=str(self.a));jobs=[subprocess.Popen([str(BIN),'record','--',str(self.a)],cwd=self.a,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE) for _ in range(12)];success=1
  for p in jobs:
   out,err=p.communicate(timeout=5);self.assertEqual(out,b'');self.assertIn(p.returncode,[0,7]);success+=p.returncode==0
  self.assertEqual(self.rows(),[(str(self.a),success)])
 def test_observer_busy_store_bounded(self):
  import time
  self.record(self.a);db=sqlite3.connect(self.root/'state/data/visits.db');db.execute('begin immediate')
  t=time.monotonic();r=self.cli('observe','--',self.a,cwd=self.a);elapsed=time.monotonic()-t
  pid=int(r.stdout.strip());self.assertGreater(pid,0);self.assertLess(elapsed,.1)
  os.kill(pid,0) # The shell must be able to guard the still-running supervisor.
  deadline=time.monotonic()+2
  while time.monotonic()<deadline:
   try:os.kill(pid,0)
   except ProcessLookupError:break
   time.sleep(.01)
  else:self.fail('supervisor did not exit after the bounded worker was killed')
  db.rollback();db.close();self.assertEqual(self.rows(),[(str(self.a),1)])
 def test_observer_marker_stays_live_until_supervisor_can_reap(self):
  import time,signal
  self.record(self.a);db=sqlite3.connect(self.root/'state/data/visits.db');db.execute('begin immediate')
  pid=None
  try:
   for _ in range(5):
    r=self.cli('observe','--',self.a,cwd=self.a);candidate=int(r.stdout.strip())
    try:os.kill(candidate,signal.SIGSTOP);pid=candidate;break
    except ProcessLookupError:continue
   self.assertIsNotNone(pid,'no live supervisor PID to suspend')
   time.sleep(.12) # The worker can finish or be killed, but cannot be reaped yet.
   os.kill(pid,0) # The shell must still see one outstanding observation.
  finally:
   if pid is not None:os.kill(pid,signal.SIGCONT)
  deadline=time.monotonic()+2
  while time.monotonic()<deadline:
   try:os.kill(pid,0)
   except ProcessLookupError:break
   time.sleep(.01)
  else:self.fail('resumed supervisor did not reap and exit')
  db.rollback();db.close();self.assertEqual(self.rows(),[(str(self.a),1)])
 def test_observer_normal_write_reaped_and_stdout_is_pid_only(self):
  import time
  self.record(self.b)
  r=self.cli('observe','--',self.a,cwd=self.a)
  pid=int(r.stdout.strip());self.assertGreater(pid,0)
  deadline=time.monotonic()+2
  while time.monotonic()<deadline:
   if dict(self.rows()).get(str(self.a))==1:break
   time.sleep(.01)
  else:self.fail('observer write did not finish')
  deadline=time.monotonic()+2
  while time.monotonic()<deadline:
   try:os.kill(pid,0)
   except ProcessLookupError:break
   time.sleep(.01)
  else:self.fail('normal supervisor did not exit')
 def test_old_generation_cannot_restore_cleared_record(self):
  self.record(self.a);db=sqlite3.connect(self.root/'state/data/visits.db');generation=db.execute("select epoch || ':' || generation from meta").fetchone()[0];db.close();self.cli('history','clear','--apply');self.cli('record','--generation',generation,'--',self.a,cwd=self.a);self.assertEqual(self.rows(),[])
 def test_corrupt_protocol_rejected_by_all_shells(self):
  fake=self.root/'fake';fake.mkdir();stub=fake/'jjump';stub.write_text('#!/bin/sh\nprintf "%s" "$JJ_BAD_OUTPUT"\nexit "$JJ_BAD_STATUS"\n');stub.chmod(0o755)
  for shell in ['bash','zsh','fish']:
   init=self.root/('bad-'+shell);init.write_bytes(self.cli('init',shell).stdout)
   for output,status in [(str(self.a)+'\n\n','0'),(str(self.a)+'\n','3'),('relative\n','0'),(str(self.a),'0')]:
    env=dict(self.env,PATH=str(fake)+':'+self.env['PATH'],JJ_BAD_OUTPUT=output,JJ_BAD_STATUS=status)
    script='source "$1"; j whatever; printf "UNCHANGED:%s\n" "$PWD"'
    argv=[shell,'-f','-c',script,'test',str(init)] if shell=='zsh' else [shell,'--noprofile','--norc','-c',script,'test',str(init)]
    if shell=='fish':argv=[shell,'--no-config','-c','source $argv[1]; j whatever; printf "UNCHANGED:%s\n" $PWD',str(init)]
    r=subprocess.run(argv,env=env,cwd=self.home,capture_output=True,timeout=5);self.assertIn(('UNCHANGED:'+str(self.home)).encode(),r.stdout,(shell,output,status,r.stderr))
 def test_concurrent_first_use_cannot_reset_schema(self):
  env=dict(self.env,PWD=str(self.a));jobs=[subprocess.Popen([str(BIN),'record','--',str(self.a)],cwd=self.a,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE) for _ in range(8)]
  for p in jobs:p.communicate(timeout=5);self.assertIn(p.returncode,[0,7])
  self.record(self.a);db=sqlite3.connect(self.root/'state/data/visits.db');self.assertEqual(db.execute('pragma user_version').fetchone()[0],4);db.close()
 def test_empty_interactive_completion_stays_local(self):
  self.record(self.a);self.cli('query','--complete','',code=4);self.cli('query','--interactive','',code=4)
 def test_effective_offline_and_credential_provenance(self):
  self.cli('config','set','semantic','on');v=json.loads(self.cli('--offline','config','show','--json').stdout);self.assertTrue(v['saved']['semantic']);self.assertFalse(v['effective']['semantic']);self.assertFalse(v['credential']['present'])
 def test_semantic_route_switch_is_opt_in_and_offline_safe(self):
  self.record(self.a)
  # The forced flag must be accepted and remain offline/local when no request is allowed.
  self.assertEqual(self.cli('--force-semantic','--offline','query','alpha').stdout,(str(self.a)+'\n').encode())
  self.cli('config','set','semantic_route','bogus',code=2)
  self.cli('config','set','semantic_route','force')
  v=json.loads(self.cli('config','show','--json').stdout);self.assertEqual(v['saved']['semantic_route'],'force')
  # Semantic off + no terminal: forced flag is inert, local result, zero network.
  self.assertEqual(self.cli('--force-semantic','query','alpha',code=5).stdout,b'')
  self.cli('config','set','semantic_route','local_first')
  v=json.loads(self.cli('config','show','--json').stdout);self.assertEqual(v['saved']['semantic_route'],'local_first')
 def test_local_match_makes_no_semantic_attempt_without_force(self):
  self.record(self.a)
  self.cli('config','set','semantic','on');self.cli('config','set','consent','always')
  r=self.cli('query','alpha')
  self.assertEqual(r.stdout,(str(self.a)+'\n').encode());self.assertNotIn(b'J5:',r.stderr)
if __name__=='__main__':unittest.main()
