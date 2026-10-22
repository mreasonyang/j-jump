import http from 'node:http';
import { lstat, readFile, realpath, stat } from 'node:fs/promises';
import path from 'node:path';
import { distRoot } from './site.mjs';

const port = 4187;
const contentTypes = {
  '.html': 'text/html; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8', '.svg': 'image/svg+xml',
  '.txt': 'text/plain; charset=utf-8', '.xml': 'application/xml; charset=utf-8',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg',
  '.webp': 'image/webp', '.avif': 'image/avif', '.gif': 'image/gif',
  '.ico': 'image/x-icon', '.woff2': 'font/woff2'
};

let root;
try {
  if ((await lstat(distRoot)).isSymbolicLink()) throw new Error('Linked output is not supported');
  root = await realpath(distRoot);
  await stat(path.join(root, 'index.html'));
} catch {
  console.error('Build the website first: npm run build');
  process.exit(1);
}

const insideRoot = (candidate) => candidate === root || candidate.startsWith(root + path.sep);
const server = http.createServer(async (request, response) => {
  response.setHeader('X-Content-Type-Options', 'nosniff');
  response.setHeader('Cache-Control', 'no-store');
  const send = (status, text) => {
    response.writeHead(status, { 'Content-Type': 'text/plain; charset=utf-8' });
    response.end(request.method === 'HEAD' ? undefined : text);
  };
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    response.setHeader('Allow', 'GET, HEAD');
    send(405, 'Method not allowed');
    return;
  }
  let pathname;
  try { pathname = decodeURIComponent(new URL(request.url, 'http://localhost').pathname); }
  catch { send(400, 'Invalid request path'); return; }
  if (pathname.includes('\0') || pathname.includes('\\') || pathname.split('/').some((part) => part.startsWith('.'))) {
    send(400, 'Invalid request path');
    return;
  }
  let candidate = path.resolve(root, '.' + pathname);
  if (!insideRoot(candidate)) { send(403, 'Forbidden'); return; }
  try {
    if ((await stat(candidate)).isDirectory()) candidate = path.join(candidate, 'index.html');
    candidate = await realpath(candidate);
    if (!insideRoot(candidate)) { send(403, 'Forbidden'); return; }
    const data = await readFile(candidate);
    response.writeHead(200, { 'Content-Type': contentTypes[path.extname(candidate).toLowerCase()] ?? 'application/octet-stream', 'Content-Length': data.length });
    response.end(request.method === 'HEAD' ? undefined : data);
  } catch (error) {
    if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') { send(500, 'Unable to read asset'); return; }
    try {
      const fallback = await realpath(path.join(root, '404.html'));
      if (!insideRoot(fallback)) { send(404, 'Not found'); return; }
      const data = await readFile(fallback);
      response.writeHead(404, { 'Content-Type': contentTypes['.html'], 'Content-Length': data.length });
      response.end(request.method === 'HEAD' ? undefined : data);
    } catch { send(404, 'Not found'); }
  }
});

server.on('error', (error) => { console.error(error.message); process.exitCode = 1; });
server.listen(port, '0.0.0.0', () => console.log(`Preview: http://127.0.0.1:${port} (listening on 0.0.0.0; website/dist only)`));
