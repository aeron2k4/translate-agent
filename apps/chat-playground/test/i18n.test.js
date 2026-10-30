import assert from "node:assert/strict";
import { test } from "node:test";
import { format } from "../js/i18n.js";

test("thay tham số và trả về khoá khi thiếu bản dịch", () => {
  const cat = { "clarify.amount_unit": "Ý bạn là {value} nghìn hay {value} triệu?" };
  assert.equal(format(cat, "clarify.amount_unit", { value: 500 }), "Ý bạn là 500 nghìn hay 500 triệu?");
  assert.equal(format(cat, "missing.key"), "missing.key");
  assert.equal(format(undefined, "x"), "x");
});
