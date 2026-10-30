// Nhãn hiển thị cạnh bản dịch lấy từ catalog của client-core (`/v1/i18n/{locale}`).

export function format(catalog, key, params = {}) {
  let s = catalog?.[key] ?? key;
  for (const [k, v] of Object.entries(params)) s = s.replaceAll(`{${k}}`, String(v));
  return s;
}
