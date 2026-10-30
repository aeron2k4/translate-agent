import assert from "node:assert/strict";
import { test } from "node:test";
import { DEV_KEY, seal } from "../js/seal.js";

test("bản mã mở được bằng khoá dev của mock enclave", async () => {
  const sealed = await seal("Cọc 5 củ nhé");
  assert.equal(sealed.key_id, "dev");
  assert.equal(sealed.nonce.length, 24);
  const bytes = (hex) => Uint8Array.from(hex.match(/../g), (b) => parseInt(b, 16));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(DEV_KEY), "AES-GCM", false, ["decrypt"]);
  const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: bytes(sealed.nonce) }, key, bytes(sealed.ciphertext));
  assert.equal(new TextDecoder().decode(plain), "Cọc 5 củ nhé");
});
