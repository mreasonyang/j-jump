import hashlib,os,pathlib,shutil,subprocess,tempfile,unittest
ROOT=pathlib.Path(__file__).resolve().parents[2]
BIN=pathlib.Path(os.environ.get('JJ_TEST_BIN',ROOT/'target/debug/jjump')).resolve()
class PackageInstaller(unittest.TestCase):
 def test_install_upgrade_reject_modified_and_uninstall(self):
  with tempfile.TemporaryDirectory() as t:
   root=pathlib.Path(t).resolve();package=root/'package';package.mkdir();shutil.copy2(BIN,package/'jjump');shutil.copy2(ROOT/'packaging/install.sh',package/'install.sh');(package/'binary.sha256').write_text(hashlib.sha256(BIN.read_bytes()).hexdigest()+'\n');prefix=root/'prefix';script=package/'install.sh'
   def run(*args,code=0):
    r=subprocess.run([str(script),'--prefix',str(prefix),*args],capture_output=True,timeout=5,env=dict(os.environ,HOME=str(root),J_JUMP_HOME=str(root/"isolated-state")));self.assertEqual(r.returncode,code,r.stderr);return r
   run();self.assertEqual(subprocess.check_output([str(prefix/'bin/jjump'),'--version']).strip(),('jjump '+(ROOT/'VERSION').read_text().strip()).encode());run('--replace');self.assertFalse((prefix/'bin/jjump.previous').exists());run('--uninstall');self.assertFalse((prefix/'bin/jjump').exists());self.assertFalse((root/'.zshrc').exists())
   run();(prefix/'bin/jjump').write_bytes(b'user-modified');run('--replace',code=2);self.assertEqual((prefix/'bin/jjump').read_bytes(),b'user-modified');run('--uninstall');self.assertFalse((prefix/'bin/jjump').exists());self.assertFalse((prefix/'share/j-jump-install/installed.sha256').exists());self.assertFalse((root/'.zshrc').exists())
 def test_bad_package_checksum_refused(self):
  with tempfile.TemporaryDirectory() as t:
   root=pathlib.Path(t).resolve();shutil.copy2(ROOT/'packaging/install.sh',root/'install.sh');(root/'jjump').write_text('fake');(root/'binary.sha256').write_text('wrong');r=subprocess.run([str(root/'install.sh'),'--prefix',str(root/'prefix')],capture_output=True);self.assertNotEqual(r.returncode,0);self.assertFalse((root/'prefix/bin/jjump').exists())
if __name__=='__main__':unittest.main()
