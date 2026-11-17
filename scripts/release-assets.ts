import { readFile, readdir, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { join } from 'node:path';

export function checksum(bytes: Buffer) {
  return createHash('sha256').update(bytes).digest('hex');
}

export async function verifyChecksums(directory: string, expected?: string[]) {
  const text = await readFile(join(directory, 'SHA256SUMS'), 'utf8');
  const entries = text.trim().split('\n').map((line) => {
    const match = /^([a-f0-9]{64})  ([A-Za-z0-9_.-]+)$/.exec(line);
    if (!match || match[2] === 'SHA256SUMS') throw new Error('Invalid checksum entry');
    return { hash: match[1], name: match[2] };
  });
  const names = entries.map(entry => entry.name).sort();
  const actual = (await readdir(directory)).filter(name => name !== 'SHA256SUMS').sort();
  if (new Set(names).size !== names.length || JSON.stringify(names) !== JSON.stringify(actual)
    || (expected && JSON.stringify(names) !== JSON.stringify([...expected].sort()))) throw new Error('Release asset set mismatch');
  for (const entry of entries) {
    if (checksum(await readFile(join(directory, entry.name))) !== entry.hash) throw new Error(`SHA256 mismatch: ${entry.name}`);
  }
}

async function main() {
  const [command, directory] = process.argv.slice(2);
  if (!directory || !['generate', 'verify'].includes(command)) throw new Error('Usage: release-assets.ts generate|verify DIRECTORY');
  if (command === 'generate') {
    const names = (await readdir(directory)).filter(name => name !== 'SHA256SUMS').sort();
    const lines = [];
    for (const name of names) {
      if (!/^[A-Za-z0-9_.-]+$/.test(name)) throw new Error('Unsafe asset filename');
      lines.push(`${checksum(await readFile(join(directory, name)))}  ${name}\n`);
    }
    await writeFile(join(directory, 'SHA256SUMS'), lines.join(''));
  }
  await verifyChecksums(directory);
  console.log('All release asset SHA256 checks passed');
}

if (process.argv[1]?.replaceAll('\\', '/').endsWith('/release-assets.ts')) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
