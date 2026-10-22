import { createHash } from 'node:crypto';
import { lstat, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { buildPublic, distRoot, siteRoot } from './site.mjs';

// ZIP's store method keeps packaging dependency-free and portable.
function crc32(buffer) {
  let crc = 0xffffffff;
  for (const byte of buffer) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

try {
  const files = await buildPublic();
  const chunks = [];
  const directory = [];
  let offset = 0;
  for (const file of files) {
    const name = Buffer.from(file, 'utf8');
    const data = await readFile(path.join(distRoot, file));
    const crc = crc32(data);
    const header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(0x800, 6);
    header.writeUInt16LE(33, 12); // Stable DOS date: 1980-01-01.
    header.writeUInt32LE(crc, 14);
    header.writeUInt32LE(data.length, 18);
    header.writeUInt32LE(data.length, 22);
    header.writeUInt16LE(name.length, 26);
    chunks.push(header, name, data);

    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x800, 8);
    central.writeUInt16LE(33, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(name.length, 28);
    central.writeUInt32LE(offset, 42);
    directory.push(central, name);
    offset += header.length + name.length + data.length;
  }
  const centralBytes = Buffer.concat(directory);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(files.length, 8);
  end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(centralBytes.length, 12);
  end.writeUInt32LE(offset, 16);
  const archive = Buffer.concat([...chunks, centralBytes, end]);
  const destination = path.join(siteRoot, 'j-jump-site.zip');
  try {
    if ((await lstat(destination)).isSymbolicLink()) throw new Error('Refusing a linked ZIP output');
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  await writeFile(destination, archive);
  console.log(`Packaged ${files.length} assets in website/j-jump-site.zip (${archive.length.toLocaleString('en-US')} bytes).`);
  console.log(`SHA-256: ${createHash('sha256').update(archive).digest('hex')}`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
