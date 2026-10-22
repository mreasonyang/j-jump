import { buildPublic } from './site.mjs';

try { await buildPublic(); }
catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
