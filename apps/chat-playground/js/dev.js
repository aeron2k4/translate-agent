// Ngăn "Công cụ": công tắc dev, phiên, tin mẫu golden và Inspector.
// Chuỗi ở đây là của công cụ dev, không phát hành cho người dùng.

import { api } from "./api.js";
import { h, icon } from "./dom.js";

export function createDevPanel(app) {
  const root = document.getElementById("dev");
  const body = h("div", { class: "dev-body" });
  const inspector = h("div", { class: "inspector" });
  const threadCode = h("code", {});
  const sampleList = h("div", { class: "sample-list" });
  let samples = [];

  const toggle = (key, label, hint) =>
    h(
      "label",
      { class: "switch-row", title: hint },
      h("span", {}, label),
      h("input", {
        type: "checkbox",
        role: "switch",
        checked: app.state.settings[key],
        onchange: (e) => {
          app.state.settings[key] = e.target.checked;
          app.render();
        },
      }),
    );

  const sampleFilter = h("input", {
    type: "search",
    placeholder: "Lọc tin mẫu (vd: cu, toi, unaccented)…",
    oninput: () => renderSamples(),
  });

  root.replaceChildren(
    h(
      "div",
      { class: "dev-head" },
      h("strong", {}, "Công cụ"),
      h("button", { class: "icon-btn", "aria-label": "Đóng", onclick: () => close() }, icon("close")),
    ),
    body,
  );
  body.append(
    section(
      "Cài đặt",
      toggle("e2e", "Mã hoá E2E (mock enclave)", "Client mã hoá tin, máy chủ chỉ thấy bản mã"),
      toggle("internal", "Hội thoại cấm gửi ra ngoài", "Chỉ dùng tầng chạy nội bộ (external_forbidden)"),
      toggle("details", "Hiện chi tiết kỹ thuật", "Tầng mô hình, cache, lỗi kiểm tra dưới mỗi tin"),
    ),
    section(
      "Phiên",
      h("div", { class: "kv" }, h("span", {}, "Luồng"), threadCode),
      h(
        "div",
        { class: "btn-row" },
        h("button", { class: "btn", onclick: () => app.addPerson() }, icon("plus", "icon xs"), " Thêm người"),
        h("button", { class: "btn", onclick: () => app.closeSession() }, "Đóng phiên"),
        h("button", { class: "btn", onclick: () => app.newSession() }, "Phiên mới"),
      ),
    ),
    section("Tin mẫu (golden, tổng hợp)", sampleFilter, sampleList),
    section("Inspector", inspector),
  );

  function section(title, ...children) {
    return h("section", { class: "dev-section" }, h("h3", {}, title), ...children);
  }

  function renderSamples() {
    const q = sampleFilter.value.trim().toLowerCase();
    const shown = samples
      .filter((s) => !q || s.text.toLowerCase().includes(q) || s.tags.some((t) => t.includes(q)))
      .slice(0, 60);
    sampleList.replaceChildren(
      ...shown.map((s) =>
        h(
          "button",
          { class: "sample", onclick: () => app.useSample(s), title: s.tags.join(", ") },
          h("span", { class: "sample-lang" }, s.source_lang),
          h("span", { class: "sample-text" }, s.text),
        ),
      ),
    );
  }

  async function loadSamples() {
    samples = await api("GET", "/v1/dev/samples").catch(() => []);
    renderSamples();
  }

  function renderSession() {
    threadCode.textContent = app.state.threadId;
  }

  async function renderInspector() {
    const msg = app.state.messages.find((m) => m.id === app.state.selected);
    if (!msg) {
      inspector.replaceChildren(h("p", { class: "muted" }, "Bấm vào một tin → Chi tiết kỹ thuật."));
      return;
    }
    const sourceLang = msg.sourceLang || app.state.participants.find((p) => p.id === msg.senderId)?.lang || "vi";
    let data = msg.inspect;
    if (!data) {
      inspector.replaceChildren(h("p", { class: "muted" }, "Đang tải…"));
      try {
        data = msg.inspect = await api("POST", "/v1/dev/inspect", { text: msg.text, source_lang: sourceLang });
      } catch (e) {
        data = { masked: `(lỗi: ${e.message})`, entities: [] };
      }
      if (app.state.selected !== msg.id) return;
    }
    const rows = data.entities.map((x) =>
      h(
        "tr",
        {},
        h("td", {}, h("code", {}, x.entity.token)),
        h("td", {}, x.entity.kind, x.pii ? h("span", { class: "pill warn" }, "PII") : null),
        h("td", {}, x.entity.source),
        h("td", {}, x.render.vi),
        h("td", {}, x.render.en),
      ),
    );
    const filter = msg.filter
      .map((f) => `${f.reader} (${f.lang}): ${f.decision.action}${f.decision.reason ? ` · ${f.decision.reason}` : ""}`)
      .join("\n");
    inspector.replaceChildren(
      h("div", { class: "kv" }, h("span", {}, "Tin"), h("code", {}, msg.id)),
      h("h4", {}, "Bản gốc"),
      h("pre", {}, msg.text),
      h("h4", {}, `Sau khi che thực thể (${sourceLang})`),
      h("pre", {}, data.masked),
      h("h4", {}, `Thực thể (${data.entities.length})`),
      data.entities.length
        ? h("table", {}, h("tr", {}, ["Token", "Loại", "Gốc", "vi", "en"].map((c) => h("th", {}, c))), rows)
        : h("p", { class: "muted" }, "Không có."),
      h("h4", {}, "Bước 1 — lọc ở client"),
      h("pre", {}, filter || "—"),
      h("h4", {}, "Response"),
      h("pre", {}, msg.response ? JSON.stringify(msg.response, null, 2) : "—"),
    );
  }

  function open() {
    document.body.classList.add("dev-open");
  }

  function close() {
    document.body.classList.remove("dev-open");
  }

  renderSession();
  renderInspector();

  return {
    open,
    close,
    toggle: () => document.body.classList.toggle("dev-open"),
    loadSamples,
    renderSession,
    renderInspector,
  };
}
