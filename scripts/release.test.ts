import test from 'node:test';
import assert from 'node:assert/strict';
import { generateKeyPairSync } from 'node:crypto';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { signBytes, verifyBytes } from './release-signing.ts';
import { makeManifest, verifyManifest } from './portable-manifest.ts';
import { checksum, verifyChecksums } from './release-assets.ts';

function identity() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  return {
    private: privateKey.export({ type: 'pkcs8', format: 'der' }).toString('base64'),
    public: Buffer.from(publicKey.export({ format: 'jwk' }).x!, 'base64url').toString('hex'),
  };
}
function executable() {
  const bytes = Buffer.alloc(1024);
  bytes.write('MZ');
  bytes.writeUInt32LE(0x80, 0x3c);
  bytes.write('PE\0\0', 0x80);
  bytes.writeUInt16LE(0x8664, 0x84);
  bytes.writeUInt16LE(0xf0, 0x94);
  bytes.writeUInt16LE(0x22, 0x96);
  bytes.writeUInt16LE(0x20b, 0x98);
  return bytes;
}
const date = '2026-11-17T00:00:00.000Z';

test('portable manifest signs exact raw bytes and final x64 EXE', () => {
  const key = identity();
  const bytes = executable();
  const raw = Buffer.from(JSON.stringify(makeManifest(bytes, '3.4.0', '更新说明', date), null, 2));
  const signature = signBytes(raw, key.private, key.public);
  const manifest = verifyManifest(raw, signature, key.public, bytes, '3.4.0');
  assert.equal(manifest.product, 'JJYB-YouTube-dl');
  assert.equal(manifest.url, 'https://github.com/jianjieyiban/JJYB-YouTube-dl/releases/download/v3.4.0/JJYB-YouTube-dl_3.4.0_x64.exe');
  assert.throws(() => verifyBytes(Buffer.concat([raw, Buffer.from('\n')]), signature, key.public));
  const changed = Buffer.from(bytes);
  changed[1023] ^= 1;
  assert.throws(() => verifyManifest(raw, signature, key.public, changed, '3.4.0'));
});

test('missing, malformed, foreign and mismatched keys fail closed', () => {
  const key = identity();
  const raw = Buffer.from('manifest');
  for (const privateKey of [undefined, '', 'invalid', 'AAAA']) assert.throws(() => signBytes(raw, privateKey, key.public));
  for (const publicKey of [undefined, '', 'ff', 'z'.repeat(64)]) assert.throws(() => signBytes(raw, key.private, publicKey));
  assert.throws(() => signBytes(raw, key.private, identity().public), /does not match/);
  const signature = signBytes(raw, key.private, key.public);
  assert.throws(() => verifyBytes(raw, signature, identity().public));
  for (const sig of ['', 'AAAA', 'not base64!', `${signature}\n`]) assert.throws(() => verifyBytes(raw, sig, key.public));
});

test('signed manifest rejects wrong identity, channel, platform, URL, hash, version and size', () => {
  const key = identity();
  const bytes = executable();
  const manifest = makeManifest(bytes, '3.4.0', 'notes', date);
  for (const [name, value] of Object.entries({ product: 'other', channel: 'installer', platform: 'windows-aarch64', url: 'https://evil.example/app.exe', sha256: '0'.repeat(64), version: '3.5.0', size: 1025 })) {
    const raw = Buffer.from(JSON.stringify({ ...manifest, [name]: value }));
    assert.throws(() => verifyManifest(raw, signBytes(raw, key.private, key.public), key.public, bytes, '3.4.0'));
  }
  assert.throws(() => verifyManifest(Buffer.from('not json'), 'AAAA', key.public, bytes, '3.4.0'), /signature/i);
});

test('generation rejects unstable versions, missing notes, invalid date and non-x64 executables', () => {
  for (const version of ['3.4.0-beta', 'v3.4.0', '03.4.0', '3.4', '../3.4.0']) assert.throws(() => makeManifest(executable(), version, 'notes', date));
  assert.throws(() => makeManifest(executable(), '3.4.0', '', date));
  assert.throws(() => makeManifest(executable(), '3.4.0', 'notes', 'invalid'));
  assert.throws(() => makeManifest(Buffer.alloc(10), '3.4.0', 'notes', date));
  for (const offset of [0, 0x84, 0x98]) {
    const bytes = executable();
    bytes[offset] = 0;
    assert.throws(() => makeManifest(bytes, '3.4.0', 'notes', date));
  }
});

test('all assets must be present, unique, named safely and hash-correct', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'jjyb-release-test-'));
  try {
    await writeFile(join(directory, 'asset.exe'), 'test');
    const sums = `${checksum(Buffer.from('test'))}  asset.exe\n`;
    await writeFile(join(directory, 'SHA256SUMS'), sums);
    await verifyChecksums(directory, ['asset.exe']);
    await assert.rejects(verifyChecksums(directory, ['missing.exe']));
    await writeFile(join(directory, 'asset.exe'), 'tampered');
    await assert.rejects(verifyChecksums(directory), /SHA256 mismatch/);
    for (const text of [sums + sums, sums.replace('asset.exe', '../asset.exe'), sums.replace('asset.exe', 'SHA256SUMS')]) {
      await writeFile(join(directory, 'SHA256SUMS'), text);
      await assert.rejects(verifyChecksums(directory));
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
