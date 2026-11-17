import { createPrivateKey, createPublicKey, sign, verify, timingSafeEqual } from 'node:crypto';

export function publicKey(hex: string | undefined) {
  if (!hex || !/^[a-fA-F0-9]{64}$/.test(hex)) throw new Error('Public key must be 32-byte hex');
  return createPublicKey({ key: { kty: 'OKP', crv: 'Ed25519', x: Buffer.from(hex, 'hex').toString('base64url') }, format: 'jwk' });
}

function base64(value: string | undefined, name: string) {
  if (!value || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) {
    throw new Error(`${name} must be canonical base64`);
  }
  return Buffer.from(value, 'base64');
}

export function verifyBytes(bytes: Uint8Array, signature: string, hex: string | undefined) {
  const sig = base64(signature, 'Signature');
  if (sig.length !== 64 || !verify(null, bytes, publicKey(hex), sig)) throw new Error('Invalid Ed25519 signature');
}

export function signBytes(bytes: Uint8Array, privateBase64: string | undefined, hex: string | undefined) {
  const der = base64(privateBase64, 'Private key');
  let key;
  try {
    key = createPrivateKey({ key: der, format: 'der', type: 'pkcs8' });
  } catch {
    throw new Error('Invalid PKCS8 private key');
  }
  if (key.asymmetricKeyType !== 'ed25519') throw new Error('Private key must be Ed25519');
  const expected = publicKey(hex).export({ format: 'der', type: 'spki' });
  const actual = createPublicKey(key).export({ format: 'der', type: 'spki' });
  if (actual.length !== expected.length || !timingSafeEqual(actual, expected)) throw new Error('Signing key pair does not match');
  const signature = sign(null, bytes, key).toString('base64');
  verifyBytes(bytes, signature, hex);
  return signature;
}
