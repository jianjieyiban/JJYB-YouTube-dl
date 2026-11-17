import { readFile, writeFile } from 'node:fs/promises';
import { signBytes } from './release-signing.ts';

async function main() {
  const bytes = await readFile('docs/manifest/manifest.json');
  const signature = signBytes(bytes, process.env.JJYB_TOOLS_PRIVATE_KEY_B64, process.env.JJYB_TOOLS_PUBLIC_KEY_HEX);
  await writeFile('docs/manifest/manifest.sig', signature);
  console.log('Tools manifest signed and key pair verified');
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
