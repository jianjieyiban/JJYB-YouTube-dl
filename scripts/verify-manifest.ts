import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { verifyBytes } from './release-signing.ts';

async function main() {
  const directory = process.argv[2] || 'docs/manifest';
  const bytes = await readFile(join(directory, 'manifest.json'));
  verifyBytes(bytes, await readFile(join(directory, 'manifest.sig'), 'utf8'), process.env.JJYB_TOOLS_PUBLIC_KEY_HEX);
  const manifest = JSON.parse(bytes.toString('utf8'));
  for (const name of ['yt-dlp', 'ffmpeg', 'ffprobe', 'deno', 'AtomicParsley']) {
    const tool = manifest.tools?.[name];
    if (!tool?.version || !tool.files?.['windows-x86_64']) throw new Error(`Missing required tool ${name}`);
    for (const file of Object.values(tool.files) as { url: string; sha256: string }[]) {
      if (!/^[a-f0-9]{64}$/i.test(file.sha256) || new URL(file.url).protocol !== 'https:') throw new Error(`Invalid tool artifact ${name}`);
    }
  }
  console.log('Tools manifest signature and required sources verified');
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
