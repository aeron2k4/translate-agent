// Mã hoá E2E giả lập: client mã hoá tin bằng khoá phiên, máy chủ chỉ thấy bản mã,
// mock enclave trong devserver mở ra. Khoá dev khớp `MockEnclave`, không dùng cho prod.

export const DEV_KEY = "xt-dev-only-key-not-for-prod!!!!";

const hex = (bytes) => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");

let devKey;

export async function seal(text, subtle = globalThis.crypto.subtle) {
  devKey ??= await subtle.importKey("raw", new TextEncoder().encode(DEV_KEY), "AES-GCM", false, [
    "encrypt",
  ]);
  const iv = globalThis.crypto.getRandomValues(new Uint8Array(12));
  const ct = await subtle.encrypt({ name: "AES-GCM", iv }, devKey, new TextEncoder().encode(text));
  return { key_id: "dev", nonce: hex(iv), ciphertext: hex(new Uint8Array(ct)) };
}
