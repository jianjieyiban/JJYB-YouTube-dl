import { execFileSync } from 'node:child_process';
import { readFile, mkdtemp, mkdir } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { verifyChecksums } from './release-assets.ts';
import { verifyManifest } from './portable-manifest.ts';
import { verifyBytes } from './release-signing.ts';

const repository = 'jianjieyiban/JJYB-YouTube-dl';
function gh(...args: string[]) {
  return execFileSync('gh', [...args, '--repo', repository], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

async function main() {
  const [kind, directory, notesPath] = process.argv.slice(2);
  if (!['portable', 'tools'].includes(kind) || !directory || !notesPath) throw new Error('Usage: publish-release.ts portable|tools DIRECTORY NOTES_FILE');
  if (process.env.GITHUB_REPOSITORY !== repository) throw new Error('Publishing is restricted to the JJYB repository');
  const { version } = JSON.parse(await readFile('package.json', 'utf8'));
  const tag = kind === 'portable' ? `v${version}` : 'tools-stable';
  const expected = kind === 'portable'
    ? [`JJYB-YouTube-dl_${version}_x64.exe`, `JJYB-YouTube-dl_${version}_x64-portable.zip`, `JJYB-YouTube-dl_${version}-source.zip`, 'portable-latest.json', 'portable-latest.json.sig']
    : ['manifest.json', 'manifest.sig'];
  await verifyChecksums(directory, expected);
  // Listing distinguishes a genuinely absent release from an authentication/network failure.
  const releases = JSON.parse(gh('release', 'list', '--limit', '1000', '--json', 'tagName,isDraft'));
  const existing = releases.find((release: { tagName: string }) => release.tagName === tag);
  if (existing && kind === 'portable' && !existing.isDraft) throw new Error('Refusing to replace a published application release');
  if (existing) {
    gh('release', 'edit', tag, '--draft=true', '--latest=false', '--notes-file', notesPath);
  } else {
    gh('release', 'create', tag, '--draft', '--latest=false', '--target', process.env.GITHUB_SHA || 'HEAD',
      '--title', kind === 'portable' ? `JJYB-YouTube-dl v${version}` : 'JJYB signed tool manifest', '--notes-file', notesPath,
      ...(kind === 'portable' ? ['--verify-tag'] : []));
  }
  const assets = [...expected, 'SHA256SUMS'];
  gh('release', 'upload', tag, ...assets.map(name => resolve(directory, name)), '--clobber');
  const uploaded = JSON.parse(gh('release', 'view', tag, '--json', 'assets,isDraft'));
  if (!uploaded.isDraft || JSON.stringify(uploaded.assets.map((asset: { name: string }) => asset.name).sort()) !== JSON.stringify([...assets].sort())) {
    throw new Error('Draft release contains unexpected or missing assets');
  }
  const workRoot = resolve('.release-work');
  await mkdir(workRoot, { recursive: true });
  const downloaded = await mkdtemp(join(workRoot, 'jjyb-release-verify-'));
  gh('release', 'download', tag, '--dir', downloaded);
  if (!(await readFile(join(directory, 'SHA256SUMS'))).equals(await readFile(join(downloaded, 'SHA256SUMS')))) throw new Error('Downloaded checksum list differs from build');
  await verifyChecksums(downloaded, expected);
  if (kind === 'portable') {
    verifyManifest(await readFile(join(downloaded, 'portable-latest.json')), await readFile(join(downloaded, 'portable-latest.json.sig'), 'utf8'),
      process.env.JJYB_UPDATE_PUBLIC_KEY_HEX, await readFile(join(downloaded, `JJYB-YouTube-dl_${version}_x64.exe`)), version);
  } else {
    verifyBytes(await readFile(join(downloaded, 'manifest.json')), await readFile(join(downloaded, 'manifest.sig'), 'utf8'), process.env.JJYB_TOOLS_PUBLIC_KEY_HEX);
  }
  // Never publish until every asset has been downloaded and compared with the build.
  gh('release', 'edit', tag, '--draft=false', `--latest=${kind === 'portable' ? 'true' : 'false'}`);
  console.log(`Verified and published ${repository} ${tag}`);
}

main().catch((error) => {
  // Do not print child-process objects/environment or signing material on failure.
  console.error(error.message);
  process.exitCode = 1;
});
