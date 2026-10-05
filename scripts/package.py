#!/usr/bin/env python3
"""Build an unsigned local archive with immutable binary and source identities."""
import argparse,gzip,hashlib,json,os,pathlib,re,shutil,subprocess,tarfile,tempfile
from release import check_binary, check_binary_disclosure, source_identity

# A licence or notice text is any file a crate ships whose name starts with one
# of these words: LICENSE/LICENSE-MIT/LICENSE-APACHE.md/LICENSE-UNICODE (the one
# `unicode-ident` requires), LICENSE-ZLIB, copies of BoringSSL/OpenSSL texts,
# COPYING, COPYRIGHT, NOTICE, UNLICENSE and similar. The previous fixed name list
# silently dropped every variant that did not match it exactly.
LICENSE_NAME=re.compile(r'^(licen[cs]e|copying|copyright|notice|unlicen[cs]e|patent)',re.I)
SKIP_DIRECTORIES={'.git','target','node_modules','dist'}
MAX_LICENSE_BYTES=1048576
# The crate root plus one directory below it: sqlcipher/LICENSE, openssl/LICENSE.txt,
# ring/third_party/fiat/LICENSE and tracing-core/src/spin/LICENSE all sit at that depth.
MAX_LICENSE_DEPTH=2
# Any crate that ships no text at all is recorded here instead of being passed off
# as covered, so the omission is machine-readable evidence in dependencies.json.
MISSING_REASON='the crate ships no licence or notice file, so its declared licence text is not redistributed with this archive'


def parse_arguments():
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--binary',type=pathlib.Path,required=True)
 p.add_argument('--target',required=True)
 p.add_argument('--dist',type=pathlib.Path,default=None,help='output directory (default: <repo>/dist)')
 p.add_argument('--release',action='store_true',help='require a clean, version-consistent source checkout and matching binary architecture')
 return p.parse_args()


def resolved_packages(metadata):
 """The dependency set actually resolved for the packaged target.

 `cargo metadata --filter-platform` prunes the per-target graph, and walking the
 resolve graph from its root keeps this inventory tied to the real locked graph
 instead of whichever crate names the build script happened to know about."""
 nodes={node['id']:node for node in metadata['resolve']['nodes']}
 by_id={package['id']:package for package in metadata['packages']}
 reached=set();pending=[metadata['resolve']['root']]
 while pending:
  package_id=pending.pop()
  if package_id in reached or package_id not in nodes:continue
  reached.add(package_id)
  pending.extend(dependency['pkg'] for dependency in nodes[package_id]['deps'])
 return [by_id[package_id] for package_id in sorted(reached) if package_id in by_id]


def license_files(package_directory):
 """Licence/notice texts a crate actually ships, as paths relative to its root."""
 package_directory=package_directory.resolve();found=[]
 def visit(directory,depth):
  try:entries=sorted(os.scandir(directory),key=lambda entry:entry.name)
  except OSError:return
  for entry in entries:
   relative=pathlib.Path(entry.path).relative_to(package_directory)
   if entry.is_dir(follow_symlinks=False):
    if depth<MAX_LICENSE_DEPTH and entry.name not in SKIP_DIRECTORIES and not entry.name.startswith('.'):visit(entry.path,depth+1)
   elif entry.is_file(follow_symlinks=False) and LICENSE_NAME.match(entry.name) and entry.stat().st_size<MAX_LICENSE_BYTES:
    found.append(relative)
 visit(package_directory,0)
 return found


def collect_licenses(packages,licenses):
 """Copy every shipped licence text and report which packages ship none."""
 inventory=[];missing=[]
 for package in packages:
  source_directory=pathlib.Path(package['manifest_path']).parent
  found=license_files(source_directory)
  inventory.append({'name':package['name'],'version':package['version'],'license':package.get('license'),'source':package.get('source'),'license_files':[str(relative) for relative in found]})
  if not found:
   missing.append({'name':package['name'],'version':package['version'],'declared_license':package.get('license'),'reason':MISSING_REASON})
   continue
  destination=licenses/(package['name']+'-'+package['version'])
  for relative in found:
   target=destination/relative;target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(source_directory/relative,target)
 return inventory,missing


p=parse_arguments();root=pathlib.Path(__file__).resolve().parents[1];version=(root/'VERSION').read_text().strip();sha=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip();out=(p.dist or root/'dist');out.mkdir(parents=True,exist_ok=True);name=f'j-jump-{version}-{p.target}';binary=p.binary.resolve();binary_hash=hashlib.sha256(binary.read_bytes()).hexdigest()
if p.release:
 try:source_identity(root);check_binary(binary.read_bytes(),p.target);check_binary_disclosure(binary.read_bytes(),[root,pathlib.Path.home()])
 except (ValueError,OSError,subprocess.CalledProcessError) as error:raise SystemExit(f'release preparation refused: {error}')
source_paths=['Cargo.toml','Cargo.lock','VERSION','src','shell','packaging','.cargo','build.rs','rust-toolchain','rust-toolchain.toml','scripts/package.py','scripts/release.py','scripts/build-release.py','scripts/ci-release.sh']
# Unrelated untracked research cannot change this archive or its compiled binary.
dirty_source=bool(subprocess.check_output(['git','status','--porcelain','--untracked-files=all','--',*source_paths],cwd=root,text=True).strip())
metadata=json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1','--filter-platform',p.target],cwd=root))
with tempfile.TemporaryDirectory() as t:
 stage=pathlib.Path(t)/name;stage.mkdir();shutil.copy2(binary,stage/'jjump');(stage/'j-jump').symlink_to('jjump');shutil.copy2(root/'packaging/install.sh',stage/'install.sh');shutil.copy2(root/'packaging/README.md',stage/'README.md');shutil.copy2(root/'LICENSE.md',stage/'LICENSE.md');(stage/'binary.sha256').write_text(binary_hash+'\n');
 (stage/'manifest.json').write_text(json.dumps({'schema_version':1,'version':version,'target':p.target,'source_sha':sha,'binary_sha256':binary_hash,'signing':'unsigned','dirty_source':dirty_source},indent=2)+'\n')
 licenses=stage/'licenses';licenses.mkdir()
 inventory,missing=collect_licenses(resolved_packages(metadata),licenses)
 # The project's own notice, both at the archive root and under licenses/, so it
 # cannot go missing again the way the reviewer found it.
 project_licenses=licenses/f'j-jump-{version}';project_licenses.mkdir(parents=True,exist_ok=True);shutil.copy2(root/'LICENSE.md',project_licenses/'LICENSE.md')
 (stage/'dependencies.json').write_text(json.dumps({'schema_version':2,'kind':'Cargo dependency graph resolved for the packaged target, including build and test dependencies','target':p.target,'package_count':len(inventory),'packages_with_license_text':len(inventory)-len(missing),'missing_license_text':missing,'packages':inventory},indent=2)+'\n')
 (stage/'licenses/MISSING.txt').write_text('Packages in this archive that ship no licence or notice file:\n'+(''.join(f"{entry['name']}-{entry['version']} ({entry['declared_license']}): {entry['reason']}\n" for entry in missing) if missing else 'none\n'))
 archive=out/(name+'.tar.gz')
 timestamp=int(subprocess.check_output(['git','show','-s','--format=%ct','HEAD'],cwd=root,text=True).strip())
 def neutral(info):
  info.uid=info.gid=0;info.uname=info.gname='';info.mtime=timestamp;info.pax_headers={}
  info.mode=0o755 if info.isdir() or info.name in {name+'/jjump',name+'/install.sh'} else 0o777 if info.issym() else 0o644
  return info
 if p.release:
  for member in stage.rglob('*'):
   if member.is_file() and not member.is_symlink():check_binary_disclosure(member.read_bytes(),[root,pathlib.Path.home()])
 with archive.open('wb') as output:
  with gzip.GzipFile(filename='',mode='wb',fileobj=output,mtime=0) as compressed:
   with tarfile.open(fileobj=compressed,mode='w') as tar:tar.add(stage,arcname=name,filter=neutral)
checksum=hashlib.sha256(archive.read_bytes()).hexdigest();(out/(name+'.tar.gz.sha256')).write_text(checksum+'  '+archive.name+'\n');print(archive);print(checksum)
