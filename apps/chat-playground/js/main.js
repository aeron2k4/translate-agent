// Chat playground: test trực quan pipeline dịch qua devserver (`make dev`).

import { api, streamTranslate } from "./api.js";
import { createDevPanel } from "./dev.js";
import { h, icon } from "./dom.js";
import { format } from "./i18n.js";
import { createPhone } from "./phone.js";
import { seal } from "./seal.js";
import {
  applyStreamEvent,
  createState,
  failTranslation,
  newMessage,
  pendingTargets,
  planTargets,
  startTranslation,
} from "./store.js";

const DOMAIN = "automotive";

const newThreadId = () => `pg-${Math.random().toString(36).slice(2, 8)}`;

const app = {
  state: createState(newThreadId()),
  catalogs: {},
  phones: new Map(),
  dev: null,

  t(lang, key, params) {
    return format(this.catalogs[lang] ?? this.catalogs.vi, key, params);
  },

  // ---------- vẽ ----------

  render() {
    for (const phone of this.phones.values()) phone.render();
    this.dev?.renderSession();
  },

  scheduleRender() {
    if (this.pending) return;
    this.pending = requestAnimationFrame(() => {
      this.pending = null;
      this.render();
    });
  },

  renderStage() {
    const ids = new Set(this.state.participants.map((p) => p.id));
    for (const id of this.phones.keys()) if (!ids.has(id)) this.phones.delete(id);
    for (const p of this.state.participants) {
      if (!this.phones.has(p.id)) this.phones.set(p.id, createPhone(this, p));
    }
    const add = h(
      "button",
      { class: "add-device", onclick: () => this.addPerson(), title: "Thêm người vào nhóm" },
      icon("plus"),
      h("span", {}, "Thêm người"),
    );
    document.getElementById("stage").replaceChildren(...[...this.phones.values()].map((x) => x.root), add);
    this.render();
  },

  // ---------- gửi & dịch ----------

  async send(sender, text) {
    const { state } = this;
    const msg = newMessage(state, sender.id, text);
    this.render();
    const readers = state.participants.filter((p) => p.id !== sender.id);
    let targets;
    try {
      // Bước 1 (mô phỏng client): mỗi người đọc tự quyết có cần dịch không.
      const decisions = await Promise.all(
        readers.map((r) => api("POST", "/v1/dev/filter", { text, reader_lang: r.lang })),
      );
      targets = planTargets(msg, readers, decisions);
    } catch (e) {
      failTranslation(msg, [...new Set(readers.map((r) => r.lang))], e.message);
      toast(`Lỗi lọc: ${e.message}`);
      this.render();
      return;
    }
    this.render();
    if (targets.length) await this.translate(msg, targets);
  },

  async translate(msg, targets, { skipClarification = false } = {}) {
    const { settings } = this.state;
    startTranslation(msg, targets);
    this.render();
    const body = {
      thread_id: this.state.threadId,
      sender_id: msg.senderId,
      text: msg.text,
      target_langs: targets,
      domain: DOMAIN,
      external_forbidden: settings.internal,
      skip_clarification: skipClarification,
    };
    // Client đã nhận diện ngôn ngữ ở bước 1 thì gửi kèm.
    if (msg.detection && msg.detection.primary !== "unknown") {
      body.source_lang = msg.detection.primary;
      body.unaccented_vi = msg.detection.unaccented_vi;
    }
    msg.sealed = settings.e2e;
    try {
      if (settings.e2e) {
        body.sealed = await seal(msg.text);
        body.text = ""; // máy chủ chỉ thấy bản mã, enclave mở ra
      }
      await streamTranslate(body, (ev) => {
        applyStreamEvent(msg, ev);
        this.scheduleRender();
      });
    } catch (e) {
      failTranslation(msg, targets, e.message);
      toast(`Lỗi dịch: ${e.message}`);
    }
    if (msg.status === "pending") failTranslation(msg, targets, "stream_incomplete");
    msg.inspect = null;
    this.render();
    if (this.state.selected === msg.id) this.dev.renderInspector();
  },

  retry(msg, lang) {
    return this.translate(msg, [lang], { skipClarification: true });
  },

  sendAnyway(msg) {
    return this.translate(msg, pendingTargets(this.state, msg), { skipClarification: true });
  },

  editAgain(msg, sender) {
    this.state.messages = this.state.messages.filter((m) => m.id !== msg.id);
    this.render();
    this.phones.get(sender.id)?.setDraft(msg.text);
  },

  toggleOriginal(msg, viewer) {
    msg.showOriginal[viewer.id] = !msg.showOriginal[viewer.id];
    this.render();
  },

  inspect(msg) {
    this.state.selected = msg.id;
    this.dev.open();
    this.dev.renderInspector();
    this.render();
  },

  useSample(sample) {
    const p = this.state.participants.find((x) => x.lang === sample.source_lang) ?? this.state.participants[0];
    this.phones.get(p.id)?.setDraft(sample.text);
  },

  // ---------- sửa bản dịch (HITL) ----------

  async submitCorrection(msg, reader, form) {
    const text = form.text.trim();
    if (!text) return;
    if (form.scope === "me") {
      msg.personalEdits[reader.id] = text; // chỉ phía client, máy chủ không lưu
    } else {
      const term = form.termSource && form.termTarget ? { source: form.termSource, target: form.termTarget } : null;
      try {
        const out = await api("POST", "/v1/corrections", {
          thread_id: this.state.threadId,
          message_id: msg.id,
          user_id: reader.id,
          scope: form.scope,
          domain: DOMAIN,
          source_lang: msg.sourceLang,
          target_lang: reader.lang,
          corrected_text: text,
          term,
          original_text: form.scope === "unit" ? msg.text : null,
        });
        if (out.suggested) toast("Đã tạo đề xuất cho đơn vị → var/review/suggestions.jsonl");
        else if (form.scope === "unit") toast("Đề xuất đơn vị cần cặp thuật ngữ; bản sửa áp dụng cho nhóm.");
      } catch (e) {
        toast(`Lỗi lưu bản sửa: ${e.message}`);
        throw e;
      }
      await this.refreshGroupEdits(msg, [reader.lang]);
    }
    this.render();
  },

  /** Hỏi lại máy chủ bản sửa nhóm; 404 nghĩa là không còn bản sửa (ví dụ đã đóng phiên). */
  async refreshGroupEdits(msg, langs = Object.keys(msg.groupEdits)) {
    for (const lang of langs) {
      try {
        const tr = await api("GET", `/v1/threads/${this.state.threadId}/messages/${msg.id}/corrected?lang=${lang}`);
        msg.groupEdits[lang] = tr.text;
      } catch {
        delete msg.groupEdits[lang];
      }
    }
  },

  async refreshAllGroupEdits() {
    await Promise.all(this.state.messages.map((m) => this.refreshGroupEdits(m)));
    this.render();
  },

  // ---------- người tham gia & phiên ----------

  addPerson() {
    const n = this.state.nextPerson++;
    this.state.participants.push({ id: `p${n}`, name: `Người ${n}`, lang: "en" });
    this.renderStage();
  },

  rename(p, name) {
    p.name = name;
    this.render();
  },

  setLang(p, lang) {
    p.lang = lang;
    this.render();
  },

  async leave(p) {
    try {
      await api("POST", `/v1/threads/${this.state.threadId}/members/${p.id}/leave`);
    } catch (e) {
      toast(`Lỗi: ${e.message}`);
      return;
    }
    for (const m of this.state.messages) if (m.senderId === p.id) m.senderName = p.name;
    this.state.participants = this.state.participants.filter((x) => x.id !== p.id);
    toast(`${p.name} rời nhóm — máy chủ đã xoá ngữ cảnh và bản sửa của luồng.`);
    this.renderStage();
    await this.refreshAllGroupEdits();
  },

  async closeSession() {
    try {
      await api("POST", `/v1/threads/${this.state.threadId}/close`);
    } catch (e) {
      toast(`Lỗi: ${e.message}`);
      return;
    }
    toast("Đã đóng phiên — ngữ cảnh và bản sửa nhóm trên máy chủ đã bị xoá.");
    await this.refreshAllGroupEdits();
  },

  newSession() {
    const { participants, settings, nextPerson } = this.state;
    this.state = { ...createState(newThreadId()), participants, settings, nextPerson };
    for (const phone of this.phones.values()) phone.closeSheet();
    this.dev.renderInspector();
    this.render();
  },
};

function toast(text) {
  const el = h("div", { class: "toast" }, text);
  document.getElementById("toasts").append(el);
  setTimeout(() => el.remove(), 4000);
}

async function loadModels() {
  const models = await api("GET", "/v1/dev/models").catch(() => null);
  if (!models) return;
  const pill = document.getElementById("env-pill");
  const external = Object.values(models).some((m) => m.external);
  const fast = models.fast;
  pill.textContent = fast ? `${fast.provider} · ${fast.model}` : "?";
  pill.classList.toggle("warn", external);
  pill.title = Object.entries(models)
    .map(([tier, m]) => `${tier}: ${m.provider} (${m.model})${m.external ? " — gửi ra ngoài" : ""}`)
    .join("\n");
}

async function init() {
  const [vi, en] = await Promise.all(["vi", "en"].map((l) => api("GET", `/v1/i18n/${l}`).catch(() => ({}))));
  app.catalogs = { vi, en };
  app.dev = createDevPanel(app);
  document.getElementById("btn-tools").addEventListener("click", () => app.dev.toggle());
  if (window.matchMedia("(min-width: 1280px)").matches) app.dev.open();
  app.renderStage();
  await Promise.all([app.dev.loadSamples(), loadModels()]);
}

init();
