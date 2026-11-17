import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { signBytes, verifyBytes } from './release-signing.ts';

export function validateExecutable(bytes: Buffer) {
  if (bytes.length < 1024 || bytes.length > 512 * 1024 * 1024) throw new Error('Executable size out of bounds');
  const offset = bytes.readUInt32LE(0x3c);
  if (bytes.subarray(0, 2).toString() !== 'MZ' || offset < 64 || offset + 24 > bytes.length
    || bytes.readUInt32LE(offset) !== 0x4550 || bytes.readUInt16LE(offset + 4) !== 0x8664) {
    throw new Error('Executable must be Windows x64 PE');
  }
  const size = bytes.readUInt16LE(offset + 20);
  const flags = bytes.readUInt16LE(offset + 22);
  if (size < 112 || offset + 24 + size > bytes.length || bytes.readUInt16LE(offset + 24) !== 0x20b
    || !(flags & 2) || (flags & 0x2000)) throw new Error('Executable must be PE32+ application');
}

export function makeManifest(bytes: Buffer, version: string, notes: string, pubDate: string) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) throw new Error('Version must be stable semver');
  if (typeof notes !== 'string' || !notes.trim()) throw new Error('Release notes required');
  if (!Number.isFinite(Date.parse(pubDate)) || new Date(pubDate).toISOString() !== pubDate) throw new Error('Publication date must be ISO UTC');
  validateExecutable(bytes);
  return {
    product: 'JJYB-YouTube-dl', platform: 'windows-x86_64', channel: 'portable', version,
    url: `https://github.com/jianjieyiban/JJYB-YouTube-dl/releases/download/v${version}/JJYB-YouTube-dl_${version}_x64.exe`,
    size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex'), notes, pub_date: pubDate,
  };
}

export function verifyManifest(raw: Buffer, signature: string, key: string | undefined, executable: Buffer, version: string) {
  verifyBytes(raw, signature, key);
  const manifest = JSON.parse(raw.toString('utf8'));
  const expected = makeManifest(executable, version, manifest.notes, manifest.pub_date);
  if (JSON.stringify(manifest) !== JSON.stringify(expected)) throw new Error('Portable manifest fields do not match executable/release');
  return manifest;
}

async function main() {
  const [command, executablePath, outputDir, notesPath] = process.argv.slice(2);
  if (!['generate', 'verify'].includes(command) || !executablePath || !outputDir) {
    throw new Error('Usage: portable-manifest.ts generate|verify EXE OUTPUT_DIR [NOTES_FILE]');
  }
  const { version } = JSON.parse(await readFile('package.json', 'utf8'));
  const bytes = await readFile(executablePath);
  const manifestPath = join(outputDir, 'portable-latest.json');
  const signaturePath = `${manifestPath}.sig`;
  if (command === 'generate') {
    if (!notesPath) throw new Error('Release notes file required');
    const manifest = makeManifest(bytes, version, await readFile(notesPath, 'utf8'), new Date().toISOString());
    const raw = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`);
    const signature = signBytes(raw, process.env.JJYB_UPDATE_PRIVATE_KEY_B64, process.env.JJYB_UPDATE_PUBLIC_KEY_HEX);
    await mkdir(outputDir, { recursive: true });
    await writeFile(manifestPath, raw);
    await writeFile(signaturePath, signature);
  }
  verifyManifest(await readFile(manifestPath), await readFile(signaturePath, 'utf8'), process.env.JJYB_UPDATE_PUBLIC_KEY_HEX, bytes, version);
  console.log('Portable manifest signature, metadata and executable verified');
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
