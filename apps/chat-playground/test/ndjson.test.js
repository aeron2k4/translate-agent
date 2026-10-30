import assert from "node:assert/strict";
import { test } from "node:test";
import { ndjsonParser } from "../js/ndjson.js";

test("ghép dòng bị cắt giữa các đoạn và bỏ dòng trống", () => {
  const events = [];
  const p = ndjsonParser((e) => events.push(e));
  p.push('{"type":"delta","te');
  p.push('xt":"a"}\n\n{"type":"done"}');
  assert.equal(events.length, 1);
  p.end();
  assert.deepEqual(events, [{ type: "delta", text: "a" }, { type: "done" }]);
});
