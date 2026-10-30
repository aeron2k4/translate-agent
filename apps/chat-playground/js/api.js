// Gọi devserver.

import { ndjsonParser } from "./ndjson.js";

async function errorOf(res) {
  const data = await res.json().catch(() => null);
  return new Error(data?.error || `HTTP ${res.status}`);
}

export async function api(method, path, body) {
  const res = await fetch(path, {
    method,
    headers: body ? { "content-type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw await errorOf(res);
  if (res.status === 204) return null;
  return res.json();
}

/** POST /v1/translate/stream, gọi `onEvent` cho từng sự kiện delta/reset/done. */
export async function streamTranslate(body, onEvent) {
  const res = await fetch("/v1/translate/stream", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw await errorOf(res);
  const parser = ndjsonParser(onEvent);
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    parser.push(decoder.decode(value, { stream: true }));
  }
  parser.push(decoder.decode());
  parser.end();
}
