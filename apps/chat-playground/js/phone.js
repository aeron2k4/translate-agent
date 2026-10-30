// Một "điện thoại": màn hình chat của một người tham gia, đọc bằng ngôn ngữ của họ.
//
// Khung (header, danh sách, ô soạn tin, lớp sheet) dựng một lần; `render()` chỉ vẽ
// lại header và danh sách tin để không mất chữ đang gõ hay sheet đang mở.

import { avatar, fmtTime, h, icon } from "./dom.js";
import { LANGS, bubbleFor, senderName } from "./store.js";

const SCOPES = ["me", "group", "unit"];

export function createPhone(app, p) {
  const t = (key, params) => app.t(p.lang, key, params);

  // ---------- chú thích phía trên máy: tên, ngôn ngữ đọc, rời nhóm ----------
  const nameInput = h("input", {
    class: "caption-name",
    value: p.name,
    "aria-label": "Tên người tham gia",
    onchange: (e) => app.rename(p, e.target.value.trim() || p.name),
  });
  const langSelect = h(
    "select",
    { class: "caption-lang", "aria-label": "Ngôn ngữ đọc", onchange: (e) => app.setLang(p, e.target.value) },
    Object.entries(LANGS).map(([code, label]) => h("option", { value: code }, label)),
  );
  const leaveBtn = h("button", { class: "caption-leave", onclick: () => app.leave(p) }, "Rời nhóm");
  const caption = h("div", { class: "device-caption" }, nameInput, langSelect, leaveBtn);

  // ---------- màn hình chat ----------
  const header = h("header", { class: "chat-header" });
  const list = h("div", { class: "chat-list" });
  const input = h("textarea", { rows: 1, placeholder: "Nhắn tin", "aria-label": "Soạn tin" });
  const sendBtn = h("button", { class: "send-btn", "aria-label": "Gửi", disabled: true }, icon("send"));
  const composer = h("div", { class: "composer" }, h("div", { class: "input-wrap" }, input), sendBtn);
  const sheetLayer = h("div", { class: "sheet-layer" });
  const screen = h("div", { class: "phone" }, header, list, composer, sheetLayer);
  const root = h("section", { class: "device" }, caption, screen);

  const autoGrow = () => {
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, 120)}px`;
    sendBtn.disabled = !input.value.trim();
  };
  const send = () => {
    const text = input.value;
    if (!text.trim()) return;
    input.value = "";
    autoGrow();
    app.send(p, text);
  };
  input.addEventListener("input", autoGrow);
  input.addEventListener("keydown", (e) => {
    // Bộ gõ tiếng Việt (IME) đang ghép chữ thì Enter thuộc về bộ gõ.
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      send();
    }
  });
  sendBtn.addEventListener("click", send);

  // ---------- sheet ----------
  function closeSheet() {
    sheetLayer.classList.remove("open");
    sheetLayer.replaceChildren();
  }

  function openSheet(...content) {
    sheetLayer.replaceChildren(
      h("div", { class: "sheet-backdrop", onclick: closeSheet }),
      h("div", { class: "sheet", role: "dialog" }, h("div", { class: "sheet-handle" }), ...content),
    );
    sheetLayer.classList.add("open");
  }

  function openActions(msg, b) {
    const items = [];
    if (b.kind === "translated") {
      items.push([
        b.showOriginal ? t("translation.view_translation") : t("translation.view_original"),
        () => app.toggleOriginal(msg, p),
      ]);
      items.push([t("translation.edit"), () => openEdit(msg, b)]);
    }
    if (b.kind === "failed") items.push([t("translation.failed_retry"), () => app.retry(msg, b.lang)]);
    if (b.kind === "untranslated") items.push([`Dịch sang ${LANGS[b.lang]}`, () => app.retry(msg, b.lang)]);
    items.push(["Chi tiết kỹ thuật", () => app.inspect(msg)]);
    openSheet(
      h("div", { class: "sheet-preview" }, b.text),
      h(
        "div",
        { class: "sheet-items" },
        items.map(([label, fn]) =>
          h("button", { class: "sheet-item", onclick: () => (closeSheet(), fn()) }, label),
        ),
      ),
      h("button", { class: "sheet-item cancel", onclick: closeSheet }, "Huỷ"),
    );
  }

  function openEdit(msg, b) {
    const form = { text: b.translated, scope: "group", termSource: "", termTarget: "" };
    const textarea = h("textarea", { class: "edit-text", oninput: (e) => (form.text = e.target.value) }, b.translated);
    const seg = h(
      "div",
      { class: "segmented", role: "radiogroup" },
      SCOPES.map((s) =>
        h(
          "button",
          {
            class: s === form.scope ? "active" : "",
            role: "radio",
            "aria-checked": String(s === form.scope),
            onclick: (e) => {
              form.scope = s;
              for (const btn of seg.children) {
                const on = btn === e.currentTarget;
                btn.classList.toggle("active", on);
                btn.setAttribute("aria-checked", String(on));
              }
            },
          },
          t(`correction.scope.${s}`),
        ),
      ),
    );
    const saveBtn = h("button", { class: "btn primary" }, "Lưu");
    saveBtn.addEventListener("click", async () => {
      saveBtn.disabled = true;
      try {
        await app.submitCorrection(msg, p, form);
        closeSheet();
      } finally {
        saveBtn.disabled = false;
      }
    });
    openSheet(
      h("div", { class: "sheet-title" }, t("translation.edit")),
      h("div", { class: "sheet-original" }, b.original),
      textarea,
      seg,
      h(
        "details",
        { class: "term-pair" },
        h("summary", {}, "Cặp thuật ngữ (tuỳ chọn, cần cho đề xuất đơn vị)"),
        h(
          "div",
          { class: "term-inputs" },
          h("input", {
            placeholder: `Thuật ngữ gốc (${msg.sourceLang ?? "?"})`,
            oninput: (e) => (form.termSource = e.target.value.trim()),
          }),
          h("input", {
            placeholder: `Bản dịch đúng (${p.lang})`,
            oninput: (e) => (form.termTarget = e.target.value.trim()),
          }),
        ),
      ),
      h("div", { class: "sheet-actions" }, h("button", { class: "btn", onclick: closeSheet }, "Huỷ"), saveBtn),
    );
    textarea.focus();
  }

  // ---------- vẽ ----------
  function renderHeader() {
    const others = app.state.participants.filter((x) => x.id !== p.id);
    const group = others.length > 1;
    const title = group ? others.map((x) => x.name).join(", ") : others[0]?.name ?? "Chưa có ai";
    const face = group
      ? h("span", { class: "avatar group" }, String(others.length + 1))
      : avatar(others[0]?.name ?? "?", others[0]?.id ?? "-");
    // replaceChildren biến null thành chữ "null": lọc trước.
    const parts = [
      h("span", { class: "header-back" }, icon("back")),
      face,
      h(
        "div",
        { class: "header-text" },
        h("div", { class: "header-title" }, title),
        h("div", { class: "header-sub" }, icon("globe", "icon xs"), ` Tự động dịch sang ${LANGS[p.lang]}`),
      ),
      app.state.settings.e2e ? h("span", { class: "header-lock", title: "Mã hoá đầu-cuối (mock)" }, icon("lock")) : null,
    ];
    header.replaceChildren(...parts.filter(Boolean));
  }

  function ownTick(status) {
    switch (status.state) {
      case "sending":
        return h("span", { class: "tick", title: "Đang gửi / đang dịch" }, icon("clock", "icon xs"));
      case "clarify":
        return h("span", { class: "tick warn", title: "Chờ làm rõ" }, icon("alert", "icon xs"));
      case "failed":
        return h("span", { class: "tick danger", title: `Dịch lỗi: ${status.langs.join(", ")}` }, icon("alert", "icon xs"));
      case "not_needed":
        return h("span", { class: "tick", title: `Không cần dịch: ${status.reasons.join(", ")}` }, icon("check", "icon xs"));
      default:
        return h(
          "span",
          { class: "tick done", title: `Đã dịch sang ${status.langs.join(", ")}` },
          icon("checks", "icon xs"),
        );
    }
  }

  function renderBubble(msg, b) {
    const time = h("span", { class: "time" }, fmtTime(msg.at));
    const bubble = (cls, ...children) =>
      h("div", { class: `bubble ${cls}`, onclick: () => clickable(b) && openActions(msg, b) }, ...children);

    switch (b.kind) {
      case "typing":
        return h("div", { class: "bubble typing", "aria-label": t("translation.translating") }, h("i"), h("i"), h("i"));
      case "own":
        return bubble("own", h("div", { class: "text" }, b.text), h("div", { class: "foot" }, time, ownTick(b.status)));
      case "streaming":
        return h("div", { class: "bubble streaming" }, h("div", { class: "text" }, b.text));
      case "plain":
        return bubble("", h("div", { class: "text" }, b.text), h("div", { class: "foot" }, time));
      case "failed":
        return bubble(
          "failed",
          h("div", { class: "text" }, b.text),
          h(
            "div",
            { class: "foot" },
            h(
              "button",
              { class: "foot-link danger", onclick: (e) => (e.stopPropagation(), app.retry(msg, b.lang)) },
              icon("retry", "icon xs"),
              " ",
              t("translation.failed_retry"),
            ),
            time,
          ),
        );
      case "untranslated":
        return bubble(
          "",
          h("div", { class: "text" }, b.text),
          h(
            "div",
            { class: "foot" },
            h(
              "button",
              { class: "foot-link", onclick: (e) => (e.stopPropagation(), app.retry(msg, b.lang)) },
              `Dịch sang ${LANGS[b.lang]}`,
            ),
            time,
          ),
        );
      default:
        return bubble(
          b.showOriginal ? "original" : "",
          h("div", { class: "text" }, b.text),
          h(
            "div",
            { class: "foot" },
            h(
              "span",
              { class: `label ${b.edited ? "human" : ""}` },
              icon(b.edited ? "pencil" : "globe", "icon xs"),
              " ",
              t(b.labelKey),
            ),
            h("span", { class: "dot" }, "·"),
            h(
              "button",
              { class: "foot-link", onclick: (e) => (e.stopPropagation(), app.toggleOriginal(msg, p)) },
              b.showOriginal ? t("translation.view_translation") : t("translation.view_original"),
            ),
            time,
          ),
        );
    }
  }

  const clickable = (b) => !["typing", "streaming"].includes(b.kind);

  function clarifyCard(msg, c) {
    const params = Object.fromEntries(c.params);
    return h(
      "div",
      { class: "clarify-card" },
      h("div", { class: "clarify-q" }, icon("alert", "icon sm"), h("span", {}, t(c.i18n_key, params))),
      h(
        "div",
        { class: "clarify-actions" },
        h("button", { class: "btn", onclick: () => app.editAgain(msg, p) }, "Sửa tin"),
        h("button", { class: "btn primary", onclick: () => app.sendAnyway(msg) }, t("clarify.send_anyway")),
      ),
    );
  }

  function techLine(msg, b) {
    const pills = [];
    const pill = (text, cls = "") => pills.push(h("span", { class: `pill ${cls}` }, text));
    if (b.kind === "own") {
      for (const tr of Object.values(msg.translations)) {
        if (!tr.streaming) pill(`→${tr.lang} ${tr.status}${tr.tier ? ` · ${tr.tier}` : ""}`, tr.status === "failed" ? "danger" : "");
      }
      for (const reason of new Set(Object.values(msg.skips))) pill(`bỏ qua: ${reason}`);
    } else {
      if (b.skip) pill(`không dịch: ${b.skip}`);
      if (b.tier) pill(b.tier);
      if (b.cached) pill("cached", "ok");
      for (const issue of b.issues ?? []) pill(issue, "warn");
      if (b.error) pill(b.error, "danger");
    }
    if (msg.sealed) pill("E2E");
    return pills.length ? h("div", { class: "tech" }, pills) : null;
  }

  function renderList() {
    const nearBottom = list.scrollHeight - list.scrollTop - list.clientHeight < 60;
    const { state } = app;
    const group = state.participants.length > 2;
    const rows = state.messages.map((m) => ({ m, b: bubbleFor(m, p) })).filter((x) => x.b);
    const items = [h("div", { class: "day-sep" }, h("span", {}, "Hôm nay"))];
    if (!rows.length) {
      items.push(h("div", { class: "empty" }, "Chưa có tin nhắn. Gõ bên dưới, hoặc chọn tin mẫu trong Công cụ."));
    }
    rows.forEach(({ m, b }, i) => {
      const own = b.kind === "own";
      const first = i === 0 || rows[i - 1].m.senderId !== m.senderId;
      const last = i === rows.length - 1 || rows[i + 1].m.senderId !== m.senderId;
      const name = senderName(state, m);
      const col = h(
        "div",
        { class: "col" },
        !own && first && group ? h("div", { class: "sender" }, name) : null,
        renderBubble(m, b),
        own && b.clarification ? clarifyCard(m, b.clarification) : null,
        own && b.status.state === "failed"
          ? h(
              "button",
              { class: "own-note danger", onclick: () => b.status.langs.forEach((l) => app.retry(m, l)) },
              `Không dịch được sang ${b.status.langs.join(", ")} · Thử lại`,
            )
          : null,
        state.settings.details ? techLine(m, b) : null,
      );
      const cls = ["row", own ? "own" : "other", first && "first", last && "last", state.selected === m.id && "selected"];
      items.push(
        h(
          "div",
          { class: cls.filter(Boolean).join(" ") },
          own ? null : last ? avatar(name, m.senderId, "avatar sm") : h("span", { class: "avatar-spacer" }),
          col,
        ),
      );
    });
    list.replaceChildren(...items);
    if (nearBottom) list.scrollTop = list.scrollHeight;
  }

  function render() {
    // Không ghi đè ô đang được sửa.
    if (document.activeElement !== nameInput) nameInput.value = p.name;
    langSelect.value = p.lang;
    leaveBtn.disabled = app.state.participants.length <= 2;
    leaveBtn.title = leaveBtn.disabled ? "Cần ít nhất 2 người" : "Máy chủ xoá ngữ cảnh và bản sửa của luồng";
    renderHeader();
    renderList();
  }

  return {
    id: p.id,
    root,
    render,
    closeSheet,
    setDraft(text) {
      input.value = text;
      autoGrow();
      input.focus();
    },
  };
}
