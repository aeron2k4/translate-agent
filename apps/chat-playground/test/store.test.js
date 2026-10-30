import assert from "node:assert/strict";
import { test } from "node:test";
import {
  applyStreamEvent,
  bubbleFor,
  createState,
  failTranslation,
  newMessage,
  ownStatus,
  pendingTargets,
  planTargets,
  startTranslation,
} from "../js/store.js";

const translate = { action: "translate", detection: { lang: "vi", primary: "vi", unaccented_vi: true } };

function setup(text = "coc 5 cu nhe") {
  const state = createState("t1");
  const [khach, tuvan] = state.participants;
  const msg = newMessage(state, khach.id, text, new Date(0));
  return { state, khach, tuvan, msg };
}

function done(lang, text, extra = {}) {
  return {
    type: "done",
    response: {
      source_lang: "vi",
      clarification: null,
      translations: [
        { lang, status: "translated", text, tier: "fast", issues: [], edited_by_human: false, label_key: "translation.by_ai", ...extra },
      ],
    },
  };
}

test("mỗi ngôn ngữ đích chỉ dịch một lần, người cùng ngôn ngữ bị bỏ qua", () => {
  const { state, khach, msg } = setup();
  state.participants.push({ id: "p3", name: "C", lang: "en" }, { id: "p4", name: "D", lang: "vi" });
  const readers = state.participants.filter((p) => p.id !== khach.id);
  const targets = planTargets(msg, readers, [
    translate,
    translate,
    { action: "skip", reason: "same_language" },
  ]);
  assert.deepEqual(targets, ["en"]);
  assert.equal(msg.skips.p4, "same_language");
  assert.equal(msg.detection.unaccented_vi, true);
  assert.deepEqual(pendingTargets(state, msg), ["en"]);
});

test("tin chỉ có emoji: không cần dịch, người gửi thấy trạng thái tương ứng", () => {
  const { tuvan, khach, msg } = setup("😂");
  const targets = planTargets(msg, [tuvan], [{ action: "skip", reason: "emoji_only" }]);
  assert.deepEqual(targets, []);
  assert.deepEqual(ownStatus(msg), { state: "not_needed", reasons: ["emoji_only"] });
  assert.deepEqual(bubbleFor(msg, tuvan), { kind: "plain", text: "😂", skip: "emoji_only" });
  assert.equal(bubbleFor(msg, khach).kind, "own");
});

test("stream: chấm chờ → chữ hiện dần → bản dịch xong có nhãn AI", () => {
  const { tuvan, msg } = setup();
  planTargets(msg, [tuvan], [translate]);
  startTranslation(msg, ["en"]);
  assert.equal(bubbleFor(msg, tuvan).kind, "typing");
  applyStreamEvent(msg, { type: "delta", lang: "en", text: "Deposit " });
  assert.deepEqual(bubbleFor(msg, tuvan), { kind: "streaming", text: "Deposit " });
  applyStreamEvent(msg, { type: "reset", lang: "en" });
  assert.equal(bubbleFor(msg, tuvan).kind, "typing");
  applyStreamEvent(msg, done("en", "Deposit 5 million VND"));
  const b = bubbleFor(msg, tuvan);
  assert.equal(b.kind, "translated");
  assert.equal(b.text, "Deposit 5 million VND");
  assert.equal(b.labelKey, "translation.by_ai");
  assert.equal(b.tier, "fast");
  assert.equal(ownStatus(msg).state, "translated");
});

test("xem bản gốc bằng một chạm, chỉ ảnh hưởng người bấm", () => {
  const { state, tuvan, msg } = setup();
  const other = { id: "p3", name: "C", lang: "en" };
  state.participants.push(other);
  startTranslation(msg, ["en"]);
  applyStreamEvent(msg, done("en", "Deposit"));
  msg.showOriginal[tuvan.id] = true;
  assert.equal(bubbleFor(msg, tuvan).text, "coc 5 cu nhe");
  assert.equal(bubbleFor(msg, other).text, "Deposit");
});

test("bản sửa: của riêng tôi > của nhóm > bản AI, luôn gắn nhãn đã chỉnh", () => {
  const { tuvan, msg } = setup();
  startTranslation(msg, ["en"]);
  applyStreamEvent(msg, done("en", "AI text"));
  msg.groupEdits.en = "Group text";
  let b = bubbleFor(msg, tuvan);
  assert.equal(b.text, "Group text");
  assert.equal(b.labelKey, "translation.edited_by_human");
  msg.personalEdits[tuvan.id] = "My text";
  b = bubbleFor(msg, tuvan);
  assert.equal(b.text, "My text");
  assert.equal(b.original, "coc 5 cu nhe");
});

test("lỗi dịch: hiện bản gốc và cho thử lại, không chặn hiển thị", () => {
  const { tuvan, msg } = setup();
  startTranslation(msg, ["en"]);
  failTranslation(msg, ["en"], "HTTP 500");
  const b = bubbleFor(msg, tuvan);
  assert.deepEqual(b, { kind: "failed", text: "coc 5 cu nhe", error: "HTTP 500", lang: "en" });
  assert.deepEqual(ownStatus(msg), { state: "failed", langs: ["en"] });
});

test("câu hỏi làm rõ: chỉ người gửi thấy, người khác chưa nhận tin", () => {
  const { khach, tuvan, msg } = setup("gia 500 dc ko");
  startTranslation(msg, ["en"]);
  const clarification = { topic: "price", i18n_key: "clarify.amount_unit", params: [["value", "500"]] };
  applyStreamEvent(msg, {
    type: "done",
    response: { source_lang: "vi", clarification, translations: [] },
  });
  assert.equal(bubbleFor(msg, tuvan), null);
  const own = bubbleFor(msg, khach);
  assert.deepEqual(own.clarification, clarification);
  assert.equal(own.status.state, "clarify");
});

test("người đọc đổi sang ngôn ngữ chưa dịch thì được mời dịch", () => {
  const { tuvan, msg } = setup();
  startTranslation(msg, ["en"]);
  applyStreamEvent(msg, done("en", "Deposit"));
  const reader = { ...tuvan, lang: "zh" };
  assert.deepEqual(bubbleFor(msg, reader), { kind: "untranslated", text: "coc 5 cu nhe", lang: "zh" });
});

test("người đọc cùng ngôn ngữ nguồn thấy bản gốc", () => {
  const { msg } = setup();
  startTranslation(msg, ["en"]);
  applyStreamEvent(msg, done("en", "Deposit"));
  const viReader = { id: "p9", name: "V", lang: "vi" };
  assert.equal(bubbleFor(msg, viReader).kind, "plain");
});

test("người vào nhóm sau không được mời dịch tin chỉ có emoji", () => {
  const { tuvan, msg } = setup("😂");
  planTargets(msg, [tuvan], [{ action: "skip", reason: "emoji_only" }]);
  const late = { id: "p3", name: "C", lang: "en" };
  assert.deepEqual(bubbleFor(msg, late), { kind: "plain", text: "😂", skip: "emoji_only" });
});
