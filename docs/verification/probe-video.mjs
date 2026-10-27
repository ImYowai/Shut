// /ws/video のH.264ストリーム検証: config受信・NALタイプ・fps・キーフレーム間隔
const url = process.argv[2] || "ws://127.0.0.1:47823/ws/video?t=0123456789abcdef0123456789abcdef";
const ws = new WebSocket(url);
ws.binaryType = "arraybuffer";
let cfg = null, n = 0, keys = 0, bytes = 0, first = 0, last = 0;
const nals = {};
ws.onmessage = (ev) => {
  if (typeof ev.data === "string") { cfg = JSON.parse(ev.data); console.log("CFG", ev.data); return; }
  const d = new Uint8Array(ev.data);
  if (!first) first = Date.now();
  last = Date.now();
  n++; bytes += d.length;
  if (d[0] & 1) keys++;
  // Annex-B の NAL type を数える
  for (let i = 1; i + 4 < d.length; i++) {
    if (d[i] === 0 && d[i + 1] === 0 && (d[i + 2] === 1 || (d[i + 2] === 0 && d[i + 3] === 1))) {
      const t = d[i + (d[i + 2] === 1 ? 3 : 4)] & 0x1f;
      nals[t] = (nals[t] || 0) + 1;
      i += 3;
    }
  }
};
setTimeout(() => {
  const sec = (last - first) / 1000;
  console.log("packets=%d keyframes=%d bytes=%d avgKB=%d fps=%d nals=%j", n, keys, bytes, Math.round(bytes / n / 1024), (n / sec).toFixed(1), nals);
  process.exit(0);
}, 6000);
