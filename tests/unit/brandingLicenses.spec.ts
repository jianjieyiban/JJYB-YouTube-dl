import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
const licenseScript = '../../scripts/licenses.ts';

describe('telemetry isolation', () => {
  it('does not embed an upstream telemetry destination', () => {
    for (const file of ['../../src/sentry.ts', '../../src-tauri/src/lib.rs']) {
      expect(readFileSync(new URL(file, import.meta.url), 'utf8')).not.toContain('ingest.us.sentry.io');
    }
  });
});

describe('third-party attribution', () => {
  it('preserves different copyright holders sharing a license and scoped package names', async () => {
    const { generateNpmLicenses } = await import(/* @vite-ignore */ licenseScript) as {
      generateNpmLicenses: (read: (path: string) => string) => string;
    };
    const result = generateNpmLicenses((path: string) => {
      const files: Record<string, string> = {
        one: 'MIT License\nCopyright Alice',
        two: 'MIT License\nCopyright Bob',
      };
      return files[path] ?? JSON.stringify({
        '@example/one@1.0.0': { licenses: 'MIT', licenseFile: 'one' },
        'two@2.0.0': { licenses: 'MIT', licenseFile: 'two' },
      });
    });
    expect(result).toContain('@example/one 1.0.0 - MIT');
    expect(result).toContain('Copyright Alice');
    expect(result).toContain('Copyright Bob');
  });
});
