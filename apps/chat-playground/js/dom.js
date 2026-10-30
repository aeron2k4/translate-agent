// Tạo DOM. Nội dung tin luôn đi qua textContent / text node, không dùng innerHTML.

export function h(tag, attrs = {}, ...children) {
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

const SVG_NS = "http://www.w3.org/2000/svg";

// Biểu tượng nét đơn, 24×24.
const ICONS = {
  send: "M3.4 20.4 21 12 3.4 3.6 3.4 10.1 15 12 3.4 13.9Z",
  back: "M15 5 8 12l7 7",
  globe:
    "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Zm-9 9h18M12 3c2.5 2.6 3.8 5.6 3.8 9s-1.3 6.4-3.8 9c-2.5-2.6-3.8-5.6-3.8-9S9.5 5.6 12 3Z",
  check: "M5 12.5 9.5 17 19 7.5",
  checks: "M2 12.5 6.5 17 16 7.5M10 15.5l1.5 1.5L21 7.5",
  clock: "M12 7v5l3 2M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z",
  alert: "M12 8v5m0 3.5v.01M10.3 3.9 2.4 18a2 2 0 0 0 1.7 3h15.8a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z",
  pencil: "M4 20h4L19 9l-4-4L4 16v4Z",
  info: "M12 11v6m0-9.5v.01M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Z",
  retry: "M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7",
  tools: "M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0M14 4v4M8 10v4M16 16v4",
  close: "M6 6l12 12M18 6 6 18",
  plus: "M12 5v14M5 12h14",
  lock: "M7 11V8a5 5 0 0 1 10 0v3M6 11h12v9H6Z",
};

export function icon(name, cls = "icon") {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("class", cls);
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS(SVG_NS, "path");
  path.setAttribute("d", ICONS[name]);
  svg.append(path);
  return svg;
}

const AVATAR_COLORS = ["#0068ff", "#7c3aed", "#059669", "#d97706", "#db2777", "#0891b2"];

export function avatar(name, seed, cls = "avatar") {
  let n = 0;
  for (const ch of seed) n = (n * 31 + ch.charCodeAt(0)) >>> 0;
  const initials = name
    .split(/\s+/)
    .filter(Boolean)
    .slice(-2)
    .map((w) => w[0].toUpperCase())
    .join("");
  return h("span", { class: cls, style: `background:${AVATAR_COLORS[n % AVATAR_COLORS.length]}` }, initials || "?");
}

export function fmtTime(d) {
  return d.toLocaleTimeString("vi-VN", { hour: "2-digit", minute: "2-digit" });
}
