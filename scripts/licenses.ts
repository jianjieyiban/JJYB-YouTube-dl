import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import type { ToolSource } from './sources.ts';
import { fetchSources, TOOL_LICENSES } from './sources.ts';

const OUTPUT_DIR = 'licenses';
const NPM_FILE = join(OUTPUT_DIR, 'licenses-npm.json');
const RUST_FILE = join(OUTPUT_DIR, 'licenses-rust.txt');
const OUTPUT_FILE = join(OUTPUT_DIR, '3rdpartylicenses.txt');

type LicenseInfo = {
  name?: string;
  version?: string;
  licenses?: string | string[];
  licenseFile?: string;
};

function readFileOrEmpty(path: string): string {
  try {
    return readFileSync(path, 'utf8').trim();
  } catch {
    console.warn(`[licenses] missing file: ${path}`);
    return '';
  }
}

async function main() {
  if (!existsSync(OUTPUT_DIR)) {
    mkdirSync(OUTPUT_DIR);
  }

  const rust = readFileOrEmpty(RUST_FILE);
  if (!rust) throw new Error('[licenses] Rust license input is required; run licenses:rust first.');
  const npm = generateNpmLicenses();
  const externalTools = await generateExternalToolsSection();

  const content = [
    'JJYB-YouTube-dl – Third-Party Licenses',
    '===========================================',
    '',
    externalTools,
    '',
    'OPENCC DICTIONARIES\n-------------------\n',
    'The conversion dictionaries bundled by ferrous-opencc 0.4.0 originate from Open Chinese Convert (OpenCC).',
    'Source: https://github.com/BYVoid/OpenCC',
    'License: Apache-2.0. The complete Apache License 2.0 text is reproduced in the Rust dependencies section below.',
    '',
    rust ? 'RUST DEPENDENCIES\n-----------------\n\n' + rust + '\n' : '',
    npm ? '\nNPM DEPENDENCIES\n----------------\n\n' + npm + '\n' : '',
    '',
  ].join('\n');

  writeFileSync(OUTPUT_FILE, content.replace(/[\t ]+$/gm, '').trimEnd() + '\n', 'utf8');
  console.log(`[licenses] wrote ${OUTPUT_FILE}`);
}

export function generateNpmLicenses(read = (path: string) => readFileSync(path, 'utf8')) {
  const raw = read(NPM_FILE);
  const data: Record<string, LicenseInfo> = JSON.parse(raw);

  const packageLines: string[] = [];
  const licenseTextMap = new Map<string, string>();

  for (const [key, info] of Object.entries(data)) {
    const separator = key.lastIndexOf('@');
    const name = info.name || (separator > 0 ? key.slice(0, separator) : key);
    const version = info.version || (separator > 0 ? key.slice(separator + 1) : 'unknown');

    const licenses = Array.isArray(info.licenses)
      ? info.licenses
      : info.licenses
        ? [info.licenses]
        : ['UNKNOWN'];

    for (const lic of licenses) {
      packageLines.push(`${name} ${version} - ${lic}`);

      if (info.licenseFile) {
        try {
          const text = read(info.licenseFile).trim();
          licenseTextMap.set(`${name} ${version} - ${lic}`, text);
        } catch (e) {
          console.warn(`[licenses] could not read license file ${info.licenseFile}: ${e}.`);
        }
      }
    }
  }

  packageLines.sort((a, b) => a.localeCompare(b));

  const licenseTextBlocks: string[] = [];
  licenseTextMap.forEach((text, id) => {
    licenseTextBlocks.push(id);
    licenseTextBlocks.push(text);
    licenseTextBlocks.push('');
    licenseTextBlocks.push('----------------------------------------');
    licenseTextBlocks.push('');
  });

  return [
    'PACKAGES',
    '========',
    '',
    packageLines.join('\n'),
    '',
    'LICENSE TEXTS',
    '============',
    '',
    licenseTextBlocks.join('\n').trim(),
    '',
  ].join('\n');
}

async function generateExternalToolsSection() {
  let sources: ToolSource[];
  try {
    sources = await fetchSources();
  } catch (err) {
    throw new Error('[licenses] Failed to fetch external tools metadata', { cause: err });
  }

  const lines: string[] = [];
  lines.push('EXTERNAL TOOLS');
  lines.push('-----------------');
  lines.push('');
  lines.push('This application may download and use external tools at runtime.');
  lines.push('These tools are separate projects with their own licenses.');
  lines.push('The versions displayed here are the ones used when this file was generated, not necessarily the current versions used by the app.');
  lines.push('');

  for (const tool of sources) {
    const meta = TOOL_LICENSES[tool.name] ?? (tool.name === 'deno'
      ? { license: 'MIT', url: 'https://github.com/denoland/deno' }
      : {});
    const license = meta.license ?? '(license unknown)';
    const site = meta.url ?? '(no project URL)';

    lines.push(`${tool.name} (${tool.version})`);
    lines.push(`  License: ${license}`);
    lines.push(`  Project: ${site}`);
    lines.push('');
  }

  return lines.join('\n');
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  void main();
}
