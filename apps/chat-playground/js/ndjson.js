// Tách luồng NDJSON (mỗi dòng một JSON) từ các đoạn văn bản nhận được.

export function ndjsonParser(onEvent) {
  let buf = "";
  const flush = (line) => {
    if (line.trim()) onEvent(JSON.parse(line));
  };
  return {
    push(text) {
      buf += text;
      let i;
      while ((i = buf.indexOf("\n")) >= 0) {
        flush(buf.slice(0, i));
        buf = buf.slice(i + 1);
      }
    },
    end() {
      flush(buf);
      buf = "";
    },
  };
}
