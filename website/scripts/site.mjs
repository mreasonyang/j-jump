import { cp, lstat, mkdir, readdir, readFile, rm, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

export const siteRoot = fileURLToPath(new URL('../', import.meta.url));
export const publicRoot = path.join(siteRoot, 'public');
export const distRoot = path.join(siteRoot, 'dist');
export const origin = 'https://j-jump.kyun.si';
const allowedExtensions = new Set(['.html', '.css', '.js', '.svg', '.txt', '.xml', '.png', '.jpg', '.jpeg', '.webp', '.avif', '.gif', '.ico', '.woff2']);
const textExtensions = new Set(['.html', '.css', '.js', '.svg', '.txt', '.xml']);
// Pages reads these reserved, extensionless files as static hosting configuration.
const hostingFiles = new Set(['_headers', '_redirects']);
const forbiddenNames = /^(?:\.|node_modules$|credentials?$|secrets?$|private$|source$|src$)/i;

export async function filesIn(root, prefix = '') {
  const files = [];
  for (const entry of await readdir(path.join(root, prefix), { withFileTypes: true })) {
    const relative = path.posix.join(prefix, entry.name);
    if (forbiddenNames.test(entry.name) || entry.isSymbolicLink()) {
      throw new Error(`Publication refuses hidden, private, source, or linked entry: ${relative}`);
    }
    if (entry.isDirectory()) files.push(...await filesIn(root, relative));
    else if (entry.isFile() && (allowedExtensions.has(path.extname(entry.name).toLowerCase()) || hostingFiles.has(relative))) files.push(relative);
    else throw new Error(`Publication refuses unsupported file: ${relative}`);
  }
  return files.sort();
}

function attributes(tag) {
  const result = {};
  for (const match of tag.matchAll(/([\w:-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/g)) {
    result[match[1].toLowerCase()] = match[2] ?? match[3] ?? match[4];
  }
  return result;
}

function references(html) {
  const refs = [];
  for (const match of html.matchAll(/<(?:a|link|script|img|source|video|audio|iframe)\b[^>]*>/gi)) {
    const attrs = attributes(match[0]);
    for (const key of ['href', 'src', 'poster']) if (attrs[key]) refs.push(attrs[key]);
    if (attrs.srcset && !attrs.srcset.startsWith('data:')) {
      refs.push(...attrs.srcset.split(',').map((item) => item.trim().split(/\s+/)[0]));
    }
  }
  return refs;
}

export async function checkPublic() {
  const errors = [];
  if ((await lstat(publicRoot)).isSymbolicLink()) throw new Error('Refusing a linked public directory');
  const files = await filesIn(publicRoot);
  const fileSet = new Set(files);
  const sources = new Map();
  for (const required of ['index.html', '404.html', 'styles.css', 'app.js', 'favicon.svg', 'robots.txt', 'sitemap.xml']) {
    if (!fileSet.has(required)) errors.push(`Missing required public asset: ${required}`);
  }
  for (const file of files) {
    if (!textExtensions.has(path.extname(file).toLowerCase()) && !hostingFiles.has(file)) continue;
    const source = await readFile(path.join(publicRoot, file), 'utf8');
    sources.set(file, source);
    if (/(?:\/Users\/|\/home\/|[A-Z]:\\Users\\|file:\/\/|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----)/i.test(source)) {
      errors.push(`${file}: private machine path or private-key material is not publishable`);
    }
    if (path.extname(file).toLowerCase() === '.js') {
      const result = spawnSync(process.execPath, ['--check', path.join(publicRoot, file)], { encoding: 'utf8' });
      if (result.error || result.status !== 0) errors.push(`${file}: JavaScript syntax check failed`);
    }
  }

  const ids = new Map([...sources].filter(([file]) => file.toLowerCase().endsWith('.html')).map(([file, source]) => {
    const found = [...source.matchAll(/\bid\s*=\s*["']([^"']+)["']/g)].map((match) => match[1]);
    if (new Set(found).size !== found.length) errors.push(`${file}: duplicate HTML id`);
    return [file, new Set(found)];
  }));

  function checkReference(from, raw) {
    const reference = raw.replaceAll('&amp;', '&');
    if (/^(?:data:|mailto:|tel:)/i.test(reference)) return;
    if (/^javascript:/i.test(reference)) {
      errors.push(`${from}: executable URL is not allowed`);
      return;
    }
    let url;
    try { url = new URL(reference, `${origin}/${from}`); }
    catch { errors.push(`${from}: invalid URL ${reference}`); return; }
    if (url.origin !== origin) return;
    let target;
    try { target = decodeURIComponent(url.pathname).replace(/^\//, ''); }
    catch { errors.push(`${from}: malformed encoded asset URL`); return; }
    if (!target || target.endsWith('/')) target += 'index.html';
    if (!fileSet.has(target)) {
      errors.push(`${from}: missing local target ${target}`);
      return;
    }
    if (url.hash && ids.has(target)) {
      let fragment;
      try { fragment = decodeURIComponent(url.hash.slice(1)); }
      catch { errors.push(`${from}: malformed anchor encoding`); return; }
      if (!ids.get(target).has(fragment)) errors.push(`${from}: missing anchor ${target}#${fragment}`);
    }
  }

  for (const [file, source] of sources) {
    if (file.toLowerCase().endsWith('.html')) {
      for (const reference of references(source)) checkReference(file, reference);
    }
    if (file.toLowerCase().endsWith('.css')) {
      for (const match of source.matchAll(/url\(\s*["']?([^"')]+)["']?\s*\)/g)) checkReference(file, match[1].trim());
    }
  }

  const index = sources.get('index.html') ?? '';
  const metadata = [...index.matchAll(/<(?:meta|link)\b[^>]*>/gi)].map((match) => attributes(match[0]));
  const canonical = metadata.find((attrs) => attrs.rel === 'canonical');
  if (canonical?.href !== `${origin}/`) errors.push('index.html: canonical must be the public HTTPS root');
  if (!/<html\b[^>]*\blang=["'][^"']+["']/i.test(index)) errors.push('index.html: missing document language');
  if (!/<title>\s*[^<]+<\/title>/i.test(index)) errors.push('index.html: missing page title');
  if (!metadata.some((attrs) => attrs.name === 'description' && attrs.content)) errors.push('index.html: missing meta description');
  if (!metadata.some((attrs) => attrs.name === 'viewport' && attrs.content?.includes('width=device-width'))) errors.push('index.html: missing responsive viewport');
  const social = new Map();
  for (const attrs of metadata) {
    const key = attrs.property ?? attrs.name;
    if (!key?.startsWith('og:') && !key?.startsWith('twitter:')) continue;
    if (social.has(key)) errors.push(`index.html: duplicate ${key}`);
    social.set(key, attrs.content);
  }
  for (const property of ['og:title', 'og:description', 'og:url', 'og:type', 'og:site_name', 'og:locale',
    'og:image', 'og:image:type', 'og:image:width', 'og:image:height', 'og:image:alt',
    'twitter:card', 'twitter:title', 'twitter:description', 'twitter:image', 'twitter:image:alt']) {
    if (!social.get(property)?.trim()) errors.push(`index.html: missing ${property}`);
  }
  if (social.get('twitter:card') !== 'summary_large_image') errors.push('index.html: Twitter card must use summary_large_image');
  if (social.get('og:url') !== `${origin}/`) errors.push('index.html: og:url must match the canonical root');
  for (const key of ['title', 'description', 'image', 'image:alt']) {
    if (social.get(`twitter:${key}`) !== social.get(`og:${key}`)) errors.push(`index.html: Twitter and Open Graph ${key} must match`);
  }
  const imageURL = `${origin}/social-card.png`;
  if (social.get('og:image') !== imageURL) errors.push('index.html: social image must use the absolute public PNG URL');
  if (social.get('og:image:type') !== 'image/png' || social.get('og:image:width') !== '1200' || social.get('og:image:height') !== '630') {
    errors.push('index.html: social image metadata must describe a 1200×630 PNG');
  }
  if (!fileSet.has('social-card.png')) errors.push('Missing required social image: social-card.png');
  else {
    const image = await readFile(path.join(publicRoot, 'social-card.png'));
    const png = image.length >= 33 && image.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
    if (!png || image.toString('ascii', 12, 16) !== 'IHDR' || image.readUInt32BE(16) !== 1200 || image.readUInt32BE(20) !== 630) {
      errors.push('social-card.png: expected an actual 1200×630 PNG');
    }
    if (image.length >= 5_000_000) errors.push('social-card.png: image must remain below 5 MB');
  }
  if (!sources.get('robots.txt')?.includes(`${origin}/sitemap.xml`)) errors.push('robots.txt: missing canonical sitemap URL');
  if (!sources.get('sitemap.xml')?.includes(`<loc>${origin}/</loc>`)) errors.push('sitemap.xml: missing canonical root URL');
  if (errors.length) throw new Error(errors.join('\n'));
  return files;
}

export async function buildPublic() {
  const files = await checkPublic();
  try {
    if ((await lstat(distRoot)).isSymbolicLink()) throw new Error('Refusing a linked dist directory');
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  await rm(distRoot, { recursive: true, force: true });
  await mkdir(distRoot, { recursive: true });
  for (const file of files) {
    const destination = path.join(distRoot, file);
    await mkdir(path.dirname(destination), { recursive: true });
    await cp(path.join(publicRoot, file), destination, { dereference: false, errorOnExist: true, force: false });
  }
  const bytes = (await Promise.all(files.map((file) => stat(path.join(distRoot, file))))).reduce((sum, item) => sum + item.size, 0);
  console.log(`Built ${files.length} public assets (${bytes.toLocaleString('en-US')} bytes) in website/dist.`);
  return files;
}
