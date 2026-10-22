import { checkPublic } from './site.mjs';

try {
  const files = await checkPublic();
  console.log(`PASS: ${files.length} public assets; JavaScript, local links, anchors, SEO, and publication boundaries.`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
