// Chat playground: test trực quan pipeline dịch qua devserver (`make dev`).
//
// Mọi nội dung tin được đưa vào DOM bằng textContent, không dùng innerHTML.
// Nhãn hiển thị cạnh bản dịch lấy từ catalog i18n của client-core; chuỗi của
// khung công cụ dev viết thẳng vì đây không phải UI phát hành.
"use strict";

const DEV_KEY = "xt-dev-only-key-not-for-prod!!!!"; // khớp MockEnclave trong devserver
const LANGS = ["vi", "en"];
const DOMAIN = "automotive";

const state = {
  threadId: newThreadId(),
  participants: [
    { id: "p1", name: "Khách", lang: "vi" },
    { id: "p2", name: "Tư vấn bán hàng", lang: "en" },
  ],
  messages: [],
  i18n: {},
  selected: null,
  editing: null, // { msgId, pid }
  e2e: false,
  internal: false,
  nextMsg: 1,
  nextPerson: 3,
};

// ---------- tiện ích ----------

function newThreadId() {
  return "pg-" + Math.random().toString(36).slice(2, 8);
}

function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "class") el.className = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (k === "value") el.value = v;
    else el.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

function t(key, lang, params = {}) {
  const cat = state.i18n[lang] || state.i18n.vi || {};
  let s = cat[key] ?? key;
  for (const [k, v] of Object.entries(params)) s = s.replaceAll(`{${k}}`, v);
  return s;
}

function toast(text) {
  const el = h("div", { class: "toast" }, text);
  document.getElementById("toasts").append(el);
  setTimeout(() => el.remove(), 4000);
}

async function api(method, path, body) {
  const res = await fetch(path, {
    method,
    headers: body ? { "content-type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  if (res.status === 204) return null;
  const data = await res.json().catch(() => null);
  if (!res.ok) throw new Error((data && data.error) || `HTTP ${res.status}`);
  return data;
}

const participant = (id) => state.participants.find((p) => p.id === id);
const message = (id) => state.messages.find((m) => m.id === id);

// ---------- mã hoá E2E giả lập (mock enclave) ----------

let devKey;
async function seal(text) {
  devKey ??= await crypto.subtle.importKey(
    "raw", new TextEncoder().encode(DEV_KEY), "AES-GCM", false, ["encrypt"]);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv }, devKey, new TextEncoder().encode(text));
  const hex = (b) => Array.from(new Uint8Array(b), (x) => x.toString(16).padStart(2, "0")).join("");
  return { key_id: "dev", nonce: hex(iv), ciphertext: hex(ct) };
}

// ---------- gửi & dịch ----------

async function streamTranslate(body, onEvent) {
  const res = await fetch("/v1/translate/stream", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => null);
    throw new Error((data && data.error) || `HTTP ${res.status}`);
  }
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, i);
      buf = buf.slice(i + 1);
      if (line.trim()) onEvent(JSON.parse(line));
    }
  }
}

async function translateMessage(msg, targets, { skipClarification = false } = {}) {
  msg.status = "pending";
  msg.clarification = null;
  for (const lang of targets) msg.translations[lang] = { text: "", streaming: true };
  renderMessages();

  const body = {
    thread_id: state.threadId,
    sender_id: msg.senderId,
    text: msg.text,
    target_langs: targets,
    domain: DOMAIN,
    external_forbidden: state.internal,
    skip_clarification: skipClarification,
  };
  // Client đã nhận diện ngôn ngữ ở bước 1 thì gửi kèm, máy chủ khỏi nhận diện lại.
  if (msg.detection && msg.detection.primary !== "unknown") {
    body.source_lang = msg.detection.primary;
    body.unaccented_vi = msg.detection.unaccented_vi;
  }
  if (state.e2e) {
    body.sealed = await seal(msg.text);
    body.text = ""; // máy chủ chỉ thấy bản mã, enclave mở ra
  }
  msg.sealed = state.e2e;

  try {
    await streamTranslate(body, (ev) => {
      const tr = msg.translations[ev.lang];
      if (ev.type === "delta" && tr) tr.text += ev.text;
      else if (ev.type === "reset" && tr) tr.text = "";
      else if (ev.type === "done") applyResponse(msg, ev.response);
      renderMessages();
    });
  } catch (e) {
    for (const lang of targets) {
      msg.translations[lang] = {
        lang, status: "failed", text: msg.text, retryable: true,
        label_key: "translation.failed_retry", error: e.message,
      };
    }
    msg.status = "done";
    toast(`Lỗi dịch: ${e.message}`);
  }
  renderMessages();
  if (state.selected === msg.id) renderInspector();
}

function applyResponse(msg, r) {
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

async function sendMessage(senderId, text) {
  const msg = {
    id: `m${state.nextMsg++}`,
    senderId,
    text,
    status: "pending",
    translations: {},
    skips: {},
    filter: [],
    personalEdits: {},
    groupEdits: {},
    showOriginal: {},
  };
  state.messages.push(msg);
  renderMessages();

  // Bước 1 (mô phỏng client): mỗi người đọc tự quyết có cần dịch không.
  const readers = state.participants.filter((p) => p.id !== senderId);
  const decisions = await Promise.all(
    readers.map((r) => api("POST", "/v1/dev/filter", { text, reader_lang: r.lang })));
  const targets = [];
  readers.forEach((r, i) => {
    const d = decisions[i];
    msg.filter.push({ reader: r.name, lang: r.lang, decision: d });
    if (d.action === "skip") msg.skips[r.id] = d.reason;
    else msg.detection ??= d.detection;
    if (d.action !== "skip" && !targets.includes(r.lang)) targets.push(r.lang);
  });
  if (!targets.length) {
    msg.status = "done";
    renderMessages();
    return;
  }
  await translateMessage(msg, targets);
}

async function translateFor(msg, lang) {
  await translateMessage(msg, [lang], { skipClarification: true });
}

// ---------- sửa bản dịch (HITL) ----------

async function submitCorrection(msg, reader, form) {
  const text = form.text.trim();
  if (!text) return;
  const scope = form.scope;
  if (scope === "me") {
    msg.personalEdits[reader.id] = text; // chỉ phía client, máy chủ không lưu
  } else {
    const term = form.termSource && form.termTarget
      ? { source: form.termSource, target: form.termTarget } : null;
    const out = await api("POST", "/v1/corrections", {
      thread_id: state.threadId,
      message_id: msg.id,
      user_id: reader.id,
      scope,
      domain: DOMAIN,
      source_lang: msg.sourceLang,
      target_lang: reader.lang,
      corrected_text: text,
      term,
      original_text: scope === "unit" ? msg.text : null,
    });
    if (out.suggested) toast("Đã tạo đề xuất cho đơn vị → var/review/suggestions.jsonl");
    else if (scope === "unit") toast("Đề xuất đơn vị cần cặp thuật ngữ; bản sửa đã áp dụng cho nhóm.");
    await refreshGroupEdits(msg);
  }
  state.editing = null;
  renderMessages();
}

async function refreshGroupEdits(msg) {
  for (const lang of Object.keys(msg.translations)) {
    try {
      const tr = await api(
        "GET", `/v1/threads/${state.threadId}/messages/${msg.id}/corrected?lang=${lang}`);
      msg.groupEdits[lang] = tr.text;
    } catch {
      delete msg.groupEdits[lang];
    }
  }
}

async function refreshAllGroupEdits() {
  await Promise.all(state.messages.map(refreshGroupEdits));
  renderMessages();
}

// ---------- render cột ----------

function renderColumns() {
  const root = document.getElementById("columns");
  root.replaceChildren(...state.participants.map(renderColumn));
  renderMessages();
}

function renderColumn(p) {
  const textarea = h("textarea", {
    placeholder: `${p.name} nhập tin (${p.lang})… Enter để gửi`,
    "data-composer": p.id,
    onkeydown: (e) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        send();
      }
    },
  });
  const send = () => {
    const text = textarea.value;
    if (!text.trim()) return;
    textarea.value = "";
    sendMessage(p.id, text).catch((e) => toast(`Lỗi: ${e.message}`));
  };
  const canLeave = state.participants.length > 2;
  return h("section", { class: "column" },
    h("div", { class: "column-head" },
      h("input", {
        value: p.name,
        "aria-label": "Tên người tham gia",
        onchange: (e) => { p.name = e.target.value || p.name; renderColumns(); },
      }),
      h("select", {
        "aria-label": "Ngôn ngữ đọc",
        onchange: (e) => { p.lang = e.target.value; renderColumns(); },
      }, LANGS.map((l) => h("option", { value: l, selected: l === p.lang }, l))),
      h("button", {
        title: canLeave ? "Thành viên rời nhóm: máy chủ xoá ngữ cảnh và bản sửa của luồng"
          : "Cần ít nhất 2 người",
        disabled: !canLeave,
        onclick: () => leave(p),
      }, "Rời nhóm"),
    ),
    h("div", { class: "messages", "data-messages": p.id }),
    h("div", { class: "composer" }, textarea, h("button", { class: "primary", onclick: send }, "Gửi")),
  );
}

function renderMessages() {
  for (const p of state.participants) {
    const list = document.querySelector(`[data-messages="${p.id}"]`);
    if (!list) continue;
    const nearBottom = list.scrollHeight - list.scrollTop - list.clientHeight < 40;
    list.replaceChildren(...state.messages.map((m) => renderMessage(m, p)).filter(Boolean));
    if (nearBottom) list.scrollTop = list.scrollHeight;
  }
}

function renderMessage(msg, viewer) {
  const own = msg.senderId === viewer.id;
  const sender = participant(msg.senderId);
  const select = () => { state.selected = msg.id; renderMessages(); renderInspector(); };
  const classes = ["msg", own && "own", state.selected === msg.id && "selected"].filter(Boolean);

  if (own) return renderOwn(msg, viewer, classes, select);
  // Tin đang chờ làm rõ chưa được gửi đi: người khác chưa thấy.
  if (msg.status === "clarify") return null;

  const lang = viewer.lang;
  const head = h("div", { class: "sender" }, sender ? sender.name : msg.senderId);

  if (msg.skips[viewer.id] || lang === msg.sourceLang) {
    const reason = msg.skips[viewer.id] || "same_language";
    return h("div", { class: classes.join(" ") }, head,
      h("div", { class: "bubble", onclick: select }, msg.text),
      h("div", { class: "meta" }, h("span", { class: "pill" }, `không dịch: ${reason}`)));
  }

  const tr = msg.translations[lang];
  if (!tr) {
    return h("div", { class: classes.join(" ") }, head,
      h("div", { class: "bubble", onclick: select }, msg.text),
      h("div", { class: "meta" },
        msg.status === "pending" ? t("translation.translating", lang) : h("button", {
          class: "link", onclick: () => translateFor(msg, lang),
        }, `Dịch sang ${lang}`)));
  }

  const personal = msg.personalEdits[viewer.id];
  const group = msg.groupEdits[lang];
  const edited = personal !== undefined || group !== undefined || tr.edited_by_human;
  const failed = tr.status === "failed";
  const showOriginal = msg.showOriginal[viewer.id];
  const shown = showOriginal || failed ? msg.text : (personal ?? group ?? tr.text);

  const bubble = h("div", {
    class: ["bubble", tr.streaming && "streaming", failed && "failed"].filter(Boolean).join(" "),
    onclick: select,
  }, shown);

  const meta = h("div", { class: "meta" });
  if (tr.streaming) {
    meta.append(t("translation.translating", lang));
  } else {
    if (edited) meta.append(h("span", { class: "label human" }, t("translation.edited_by_human", lang)));
    else if (tr.label_key && !failed) meta.append(h("span", { class: "label" }, t(tr.label_key, lang)));
    if (failed) {
      meta.append(h("button", { class: "link", onclick: () => translateFor(msg, lang) },
        t("translation.failed_retry", lang)));
    } else {
      meta.append(h("button", {
        class: "link",
        onclick: () => { msg.showOriginal[viewer.id] = !showOriginal; renderMessages(); },
      }, showOriginal ? t("translation.view_translation", lang) : t("translation.view_original", lang)));
      meta.append(h("button", {
        class: "link",
        onclick: () => { state.editing = { msgId: msg.id, pid: viewer.id }; renderMessages(); },
      }, t("translation.edit", lang)));
    }
    if (tr.tier) meta.append(h("span", { class: "pill" }, tr.tier));
    if (tr.status === "cached") meta.append(h("span", { class: "pill ok" }, "cached"));
    if (failed) meta.append(h("span", { class: "pill danger" }, tr.error || "failed"));
    for (const issue of tr.issues || []) meta.append(h("span", { class: "pill warn" }, issue));
    if (msg.sealed) meta.append(h("span", { class: "pill" }, "E2E"));
  }

  const editing = state.editing && state.editing.msgId === msg.id && state.editing.pid === viewer.id;
  return h("div", { class: classes.join(" ") }, head, bubble, meta,
    editing ? renderEditor(msg, viewer, personal ?? group ?? tr.text) : null);
}

function renderOwn(msg, viewer, classes, select) {
  const lang = viewer.lang;
  const meta = h("div", { class: "meta" });
  if (msg.status === "pending") meta.append("đang gửi…");
  for (const [l, tr] of Object.entries(msg.translations)) {
    if (tr.streaming) continue;
    const cls = tr.status === "failed" ? "pill danger" : "pill ok";
    meta.append(h("span", { class: cls }, `→ ${l}: ${tr.status}${tr.tier ? ` (${tr.tier})` : ""}`));
  }
  if (msg.status === "done" && !Object.keys(msg.translations).length) {
    const reasons = [...new Set(Object.values(msg.skips))];
    meta.append(h("span", { class: "pill" }, `không cần dịch: ${reasons.join(", ") || "—"}`));
  }
  if (msg.sealed) meta.append(h("span", { class: "pill" }, "E2E"));

  let clarify = null;
  if (msg.status === "clarify" && msg.clarification) {
    const c = msg.clarification;
    const params = Object.fromEntries(c.params);
    clarify = h("div", { class: "clarify" },
      h("p", {}, t(c.i18n_key, lang, params)),
      h("div", { class: "actions" },
        h("button", {
          onclick: () => {
            state.messages = state.messages.filter((m) => m.id !== msg.id);
            const box = document.querySelector(`[data-composer="${viewer.id}"]`);
            if (box) { box.value = msg.text; box.focus(); }
            renderMessages();
          },
        }, "Sửa tin"),
        h("button", {
          class: "primary",
          onclick: () => {
            const targets = [...new Set(state.participants
              .filter((p) => p.id !== msg.senderId && !msg.skips[p.id]).map((p) => p.lang))];
            translateMessage(msg, targets, { skipClarification: true });
          },
        }, t("clarify.send_anyway", lang)),
      ));
  }
  return h("div", { class: classes.join(" ") },
    h("div", { class: "bubble", onclick: select }, msg.text), meta, clarify);
}

function renderEditor(msg, reader, current) {
  const lang = reader.lang;
  const form = { text: current, scope: "group", termSource: "", termTarget: "" };
  const scopes = ["me", "group", "unit"];
  return h("div", { class: "editor" },
    h("textarea", { oninput: (e) => { form.text = e.target.value; } }, current),
    h("div", { class: "row" }, scopes.map((s) => h("label", {},
      h("input", {
        type: "radio", name: `scope-${msg.id}-${reader.id}`, value: s, checked: s === form.scope,
        onchange: () => { form.scope = s; },
      }), " ", t(`correction.scope.${s}`, lang)))),
    h("div", { class: "row" },
      h("input", { placeholder: `Thuật ngữ gốc (${msg.sourceLang})`, oninput: (e) => { form.termSource = e.target.value.trim(); } }),
      h("input", { placeholder: `Bản dịch đúng (${lang})`, oninput: (e) => { form.termTarget = e.target.value.trim(); } })),
    h("div", { class: "row" },
      h("button", {
        class: "primary",
        onclick: () => submitCorrection(msg, reader, form).catch((e) => toast(`Lỗi: ${e.message}`)),
      }, "Lưu"),
      h("button", { onclick: () => { state.editing = null; renderMessages(); } }, "Huỷ")),
  );
}

// ---------- inspector ----------

async function renderInspector() {
  const root = document.getElementById("inspector");
  const msg = message(state.selected);
  if (!msg) return;
  const sourceLang = msg.sourceLang || participant(msg.senderId)?.lang || "vi";
  root.replaceChildren(h("h2", {}, `Inspector · ${msg.id}`), h("p", { class: "muted" }, "Đang tải…"));
  let inspect;
  try {
    inspect = msg.inspect ??= await api("POST", "/v1/dev/inspect", { text: msg.text, source_lang: sourceLang });
  } catch (e) {
    inspect = { masked: `(lỗi: ${e.message})`, entities: [] };
  }
  if (state.selected !== msg.id) return;

  const rows = inspect.entities.map((x) => h("tr", {},
    h("td", {}, h("code", {}, x.entity.token)),
    h("td", {}, x.entity.kind, x.pii ? h("span", { class: "pill warn" }, "PII") : null),
    h("td", {}, x.entity.source),
    h("td", {}, x.render.vi),
    h("td", {}, x.render.en)));

  root.replaceChildren(
    h("h2", {}, `Inspector · ${msg.id}`),
    h("h3", {}, "Bản gốc"), h("pre", {}, msg.text),
    h("h3", {}, `Sau khi che thực thể (nguồn: ${sourceLang})`), h("pre", {}, inspect.masked),
    h("h3", {}, `Thực thể (${inspect.entities.length})`),
    inspect.entities.length
      ? h("table", {}, h("tr", {}, ["Token", "Loại", "Gốc", "vi", "en"].map((c) => h("th", {}, c))), rows)
      : h("p", { class: "muted" }, "Không có."),
    h("h3", {}, "Bước 1 — lọc ở client"),
    h("pre", {}, msg.filter.map((f) =>
      `${f.reader} (${f.lang}): ${f.decision.action}${f.decision.reason ? " · " + f.decision.reason : ""}`).join("\n") || "—"),
    h("h3", {}, "Response"),
    h("pre", {}, msg.response ? JSON.stringify(msg.response, null, 2) : "—"),
  );
}

// ---------- phiên ----------

async function leave(p) {
  await api("POST", `/v1/threads/${state.threadId}/members/${p.id}/leave`);
  state.participants = state.participants.filter((x) => x.id !== p.id);
  toast(`${p.name} rời nhóm — máy chủ đã xoá ngữ cảnh và bản sửa của luồng.`);
  renderColumns();
  await refreshAllGroupEdits();
}

async function closeSession() {
  await api("POST", `/v1/threads/${state.threadId}/close`);
  toast("Đã đóng phiên — ngữ cảnh và bản sửa nhóm trên máy chủ đã bị xoá.");
  await refreshAllGroupEdits();
}

function newSession() {
  state.threadId = newThreadId();
  state.messages = [];
  state.selected = null;
  state.editing = null;
  document.getElementById("thread-id").textContent = state.threadId;
  document.getElementById("inspector").replaceChildren(h("h2", {}, "Inspector"),
    h("p", { class: "muted" }, "Bấm vào một tin để xem chi tiết."));
  renderColumns();
}

// ---------- khởi động ----------

async function loadSamples() {
  const select = document.getElementById("sample-select");
  const samples = await api("GET", "/v1/dev/samples").catch(() => []);
  const groups = {};
  for (const s of samples) (groups[s.source_lang] ??= []).push(s);
  for (const [lang, items] of Object.entries(groups)) {
    select.append(h("optgroup", { label: `Nguồn ${lang} (${items.length})` },
      items.map((s) => h("option", { value: s.id }, `${s.text}   · ${s.tags.join(", ")}`))));
  }
  select.addEventListener("change", () => {
    const s = samples.find((x) => x.id === select.value);
    select.value = "";
    if (!s) return;
    const p = state.participants.find((x) => x.lang === s.source_lang) || state.participants[0];
    const box = document.querySelector(`[data-composer="${p.id}"]`);
    if (box) { box.value = s.text; box.focus(); }
  });
}

async function init() {
  document.getElementById("thread-id").textContent = state.threadId;
  const [vi, en] = await Promise.all(LANGS.map((l) => api("GET", `/v1/i18n/${l}`).catch(() => ({}))));
  state.i18n = { vi, en };

  document.getElementById("opt-e2e").addEventListener("change", (e) => { state.e2e = e.target.checked; });
  document.getElementById("opt-internal").addEventListener("change", (e) => { state.internal = e.target.checked; });
  document.getElementById("btn-add").addEventListener("click", () => {
    const n = state.nextPerson++;
    state.participants.push({ id: `p${n}`, name: `Người ${n}`, lang: "en" });
    renderColumns();
  });
  document.getElementById("btn-close").addEventListener("click", () => closeSession().catch((e) => toast(e.message)));
  document.getElementById("btn-new").addEventListener("click", newSession);

  renderColumns();
  await Promise.all([loadSamples(), loadModels()]);
}

async function loadModels() {
  const models = await api("GET", "/v1/dev/models").catch(() => null);
  if (!models) return;
  const pill = document.getElementById("env-pill");
  const external = Object.values(models).some((m) => m.external);
  const desc = Object.entries(models)
    .map(([tier, m]) => `${tier}: ${m.provider}${m.external ? " (ra ngoài)" : ""}`).join(" · ");
  pill.textContent = `mock enclave · ${desc}`;
  pill.title = external
    ? "Tin được gửi tới provider bên ngoài — chỉ dùng dữ liệu tổng hợp"
    : "Mọi tầng chạy nội bộ (provider giả)";
}

init();
