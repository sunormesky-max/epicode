import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const appDirectory = fileURLToPath(new URL('.', import.meta.url));
const flatDistDirectory = resolve(appDirectory, 'dist');
const nestedDistDirectory = resolve(flatDistDirectory, 'public');

async function isFile(path) {
  try {
    return (await stat(path)).isFile();
  } catch (error) {
    const code = error && typeof error === 'object' && 'code' in error ? error.code : undefined;
    if (code === 'ENOENT' || code === 'ENOTDIR') return false;
    throw error;
  }
}

const distDirectory = await isFile(resolve(flatDistDirectory, 'index.html'))
  ? flatDistDirectory
  : nestedDistDirectory;
const indexFile = resolve(distDirectory, 'index.html');
if (!await isFile(indexFile)) throw new Error(`Frontend build index is missing: ${indexFile}`);
const contentTypes = new Map([
  ['.css', 'text/css; charset=utf-8'],
  ['.html', 'text/html; charset=utf-8'],
  ['.ico', 'image/x-icon'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.json', 'application/json; charset=utf-8'],
  ['.map', 'application/json; charset=utf-8'],
  ['.mjs', 'text/javascript; charset=utf-8'],
  ['.png', 'image/png'],
  ['.svg', 'image/svg+xml'],
  ['.txt', 'text/plain; charset=utf-8'],
  ['.wasm', 'application/wasm'],
  ['.webp', 'image/webp'],
  ['.woff2', 'font/woff2'],
  ['.xml', 'application/xml; charset=utf-8'],
]);

function sendText(response, status, message) {
  response.writeHead(status, {
    'Content-Type': 'text/plain; charset=utf-8',
    'X-Content-Type-Options': 'nosniff',
  });
  response.end(message);
}

const server = createServer(async (request, response) => {
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    response.setHeader('Allow', 'GET, HEAD');
    sendText(response, 405, 'Method not allowed');
    return;
  }

  let pathname;
  try {
    pathname = decodeURIComponent(new URL(request.url ?? '/', 'http://localhost').pathname);
  } catch {
    sendText(response, 400, 'Invalid request path');
    return;
  }

  if (pathname.includes('\0') || pathname.includes('\\')) {
    sendText(response, 400, 'Invalid request path');
    return;
  }
  if (pathname === '/api' || pathname.startsWith('/api/')) {
    sendText(response, 404, 'API routes are served by the gateway');
    return;
  }

  let file = resolve(distDirectory, `.${pathname}`);
  if (file !== distDirectory && !file.startsWith(`${distDirectory}${sep}`)) {
    sendText(response, 400, 'Invalid request path');
    return;
  }

  try {
    const fileInfo = await stat(file);
    if (!fileInfo.isFile()) file = indexFile;
  } catch (error) {
    const code = error && typeof error === 'object' && 'code' in error ? error.code : undefined;
    if (code !== 'ENOENT' && code !== 'ENOTDIR') {
      console.error(`Failed to inspect ${file}:`, error);
      sendText(response, 500, 'Unable to serve this page');
      return;
    }
    if (extname(pathname)) {
      sendText(response, 404, 'Not found');
      return;
    }
    file = indexFile;
  }

  try {
    const body = await readFile(file);
    const extension = extname(file).toLowerCase();
    response.writeHead(200, {
      'Cache-Control': file === indexFile ? 'no-cache' : 'public, max-age=3600',
      'Content-Length': body.byteLength,
      'Content-Type': contentTypes.get(extension) ?? 'application/octet-stream',
      'Referrer-Policy': 'strict-origin-when-cross-origin',
      'X-Content-Type-Options': 'nosniff',
    });
    response.end(request.method === 'HEAD' ? undefined : body);
  } catch (error) {
    console.error(`Failed to serve ${file}:`, error);
    sendText(response, 500, 'Unable to serve this page');
  }
});

const host = process.env.HOST ?? '0.0.0.0';
const port = Number(process.env.PORT ?? '3000');
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  console.error(`Invalid PORT: ${process.env.PORT}`);
  process.exit(1);
}

server.listen(port, host, () => {
  console.log(`Epicode frontend listening on ${host}:${port}`);
});
