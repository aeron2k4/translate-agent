// Trạng thái hội thoại và quy tắc hiển thị bong bóng. Thuần logic: không chạm
// DOM hay mạng, để test bằng `node --test`.

export const LANGS = { vi: "Tiếng Việt", en: "English" };

export function createState(threadId) {
  return {
    threadId,
    participants: [
      { id: "p1", name: "Khách", lang: "vi" },
      { id: "p2", name: "Tư vấn bán hàng", lang: "en" },
    ],
    messages: [],
    nextMsg: 1,
    nextPerson: 3,
    selected: null,
    settings: { e2e: false, internal: false, details: false },
  };
}

export function newMessage(state, senderId, text, at = new Date()) {
  const msg = {
    id: `m${state.nextMsg++}`,
    senderId,
    text,
    at,
    // filtering → pending → done | clarify
    status: "filtering",
    sourceLang: null,
    detection: null,
    translations: {}, // lang → bản dịch (đang stream hoặc đã xong)
    skips: {}, // id người đọc → lý do không dịch
    filter: [], // quyết định bước 1 của từng người đọc (cho Inspector)
    personalEdits: {}, // id người đọc → bản sửa "chỉ tôi" (chỉ phía client)
    groupEdits: {}, // lang → bản sửa của nhóm (lấy từ máy chủ)
    showOriginal: {}, // id người đọc → đang xem bản gốc
    response: null,
    clarification: null,
    sealed: false,
  };
  state.messages.push(msg);
  return msg;
}

/** Bước 1: từ quyết định lọc của từng người đọc ra danh sách ngôn ngữ đích (mỗi ngôn ngữ một lần). */
export function planTargets(msg, readers, decisions) {
  const targets = [];
  readers.forEach((reader, i) => {
    const d = decisions[i];
    msg.filter.push({ reader: reader.name, lang: reader.lang, decision: d });
    if (d.action === "skip") {
      msg.skips[reader.id] = d.reason;
      return;
    }
    msg.detection ??= d.detection;
    if (!targets.includes(reader.lang)) targets.push(reader.lang);
  });
  if (!targets.length) msg.status = "done";
  return targets;
}

/** Ngôn ngữ đích khi người gửi chọn "gửi như hiện tại" sau câu hỏi làm rõ. */
export function pendingTargets(state, msg) {
  const langs = state.participants
    .filter((p) => p.id !== msg.senderId && !msg.skips[p.id])
    .map((p) => p.lang);
  return [...new Set(langs)];
}

export function startTranslation(msg, targets) {
  msg.status = "pending";
  msg.clarification = null;
  for (const lang of targets) msg.translations[lang] = { lang, text: "", streaming: true };
}

export function applyStreamEvent(msg, ev) {
  const tr = msg.translations[ev.lang];
  if (ev.type === "delta" && tr) tr.text += ev.text;
  else if (ev.type === "reset" && tr) tr.text = "";
  else if (ev.type === "done") applyResponse(msg, ev.response);
}

export function applyResponse(msg, r) {
  msg.response = r;
  msg.sourceLang = r.source_lang;
  if (r.clarification) {
    msg.status = "clarify";
    msg.clarification = r.clarification;
    msg.translations = {};
    return;
  }
  for (const tr of r.translations) msg.translations[tr.lang] = { ...tr, streaming: false };
  msg.status = "done";
}

/** Lỗi mạng/máy chủ: luôn trả bản gốc kèm "Thử dịch lại", không chặn hiển thị. */
export function failTranslation(msg, targets, error) {
  for (const lang of targets) {
    msg.translations[lang] = {
      lang,
      status: "failed",
      text: msg.text,
      retryable: true,
      streaming: false,
      error,
    };
  }
  msg.status = "done";
}

/** Trạng thái tin nhìn từ phía người gửi. */
export function ownStatus(msg) {
  if (msg.status === "filtering" || msg.status === "pending") return { state: "sending" };
  if (msg.status === "clarify") return { state: "clarify" };
  const trs = Object.values(msg.translations);
  if (!trs.length) {
    return { state: "not_needed", reasons: [...new Set(Object.values(msg.skips))] };
  }
  if (trs.some((t) => t.status === "failed")) {
    return { state: "failed", langs: trs.filter((t) => t.status === "failed").map((t) => t.lang) };
  }
  return { state: "translated", langs: trs.map((t) => t.lang), tiers: trs.map((t) => t.tier) };
}

/**
 * Bong bóng mà `viewer` thấy cho tin `msg`, hoặc null nếu viewer chưa thấy tin
 * (tin đang chờ người gửi trả lời câu hỏi làm rõ thì chưa được gửi đi).
 */
export function bubbleFor(msg, viewer) {
  if (msg.senderId === viewer.id) {
    return {
      kind: "own",
      text: msg.text,
      status: ownStatus(msg),
      clarification: msg.status === "clarify" ? msg.clarification : null,
    };
  }
  if (msg.status === "clarify") return null;

  const lang = viewer.lang;
  const skip = msg.skips[viewer.id];
  if (skip || (msg.sourceLang && msg.sourceLang === lang)) {
    return { kind: "plain", text: msg.text, skip: skip || "same_language" };
  }
  const tr = msg.translations[lang];
  if (!tr) {
    if (msg.status !== "done") return { kind: "typing" };
    // Tin không cần dịch với bất kỳ ai (emoji, URL, mã số…) thì người vào sau cũng vậy.
    const reasons = Object.values(msg.skips);
    if (!Object.keys(msg.translations).length && reasons.length && !reasons.includes("same_language")) {
      return { kind: "plain", text: msg.text, skip: reasons[0] };
    }
    // Người đọc đổi ngôn ngữ hoặc vào nhóm sau khi tin đã dịch xong.
    return { kind: "untranslated", text: msg.text, lang };
  }
  if (tr.streaming) return tr.text ? { kind: "streaming", text: tr.text } : { kind: "typing" };
  if (tr.status === "failed") return { kind: "failed", text: msg.text, error: tr.error, lang };

  const personal = msg.personalEdits[viewer.id];
  const group = msg.groupEdits[lang];
  const edited = personal !== undefined || group !== undefined || Boolean(tr.edited_by_human);
  const translated = personal ?? group ?? tr.text;
  const showOriginal = Boolean(msg.showOriginal[viewer.id]);
  return {
    kind: "translated",
    text: showOriginal ? msg.text : translated,
    translated,
    original: msg.text,
    showOriginal,
    edited,
    labelKey: edited ? "translation.edited_by_human" : tr.label_key || "translation.by_ai",
    tier: tr.tier,
    cached: tr.status === "cached",
    issues: tr.issues || [],
    lang,
  };
}

/** Tên người gửi; người đã rời nhóm vẫn hiện tên cũ nếu còn biết. */
export function senderName(state, msg) {
  return state.participants.find((p) => p.id === msg.senderId)?.name ?? msg.senderName ?? "?";
}
