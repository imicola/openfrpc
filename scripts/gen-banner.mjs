// OpenFrpc release 头图：纯 Node 光栅化（标记），文字部分由 PowerShell/GDI+ 后处理
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const OUT = join(process.cwd(), "docs");
mkdirSync(OUT, { recursive: true });

const W = 1280, H = 640;
const BG = [22, 32, 45];       // #16202D
const CYAN = [111, 211, 238];  // #6FD3EE
const WHITE = [255, 255, 255];

function inOpenRing(x, y, cx, cy, R, w, gapDeg) {
  const d = Math.hypot(x - cx, y - cy);
  if (Math.abs(d - R) >= w) return false;
  const deg = (Math.atan2(y - cy, x - cx) * 180) / Math.PI;
  return !(deg > -gapDeg && deg < gapDeg);
}
function inShaft(x, y, cy, x0, x1, halfH) {
  return x > x0 && x < x1 && Math.abs(y - cy) < halfH;
}
function inHead(x, y, cy, baseX, tipX, halfH) {
  if (x < baseX || x > tipX) return false;
  const t = (x - baseX) / (tipX - baseX);
  return Math.abs(y - cy) < halfH * (1 - t);
}

// 主标记：无外框，裸的开口环 + 箭头，中心 (470, 320)，环 R=150
const MARK = { cx: 330, cy: 320, R: 150, w: 44, gap: 36, shaftX0: 105, shaftX1: 448, halfH: 26, headBase: 405, headTip: 585, headHalf: 105 };
// 装饰：右上与左下的暗淡大环
const DECOR = [
  { cx: 1190, cy: 520, R: 300, w: 34, alpha: 0.07 },
  { cx: -60, cy: 640, R: 240, w: 30, alpha: 0.05 },
];

function sample(x, y) {
  // 装饰环（无缺口，低透明度青）
  for (const d of DECOR) {
    if (Math.abs(Math.hypot(x - d.cx, y - d.cy) - d.R) < d.w) {
      return { c: CYAN, a: d.alpha };
    }
  }
  const m = MARK;
  if (inShaft(x, y, m.cy, m.shaftX0, m.shaftX1, m.halfH) || inHead(x, y, m.cy, m.headBase, m.headTip, m.headHalf)) {
    return { c: WHITE, a: 1 };
  }
  if (inOpenRing(x, y, m.cx, m.cy, m.R, m.w, m.gap)) {
    return { c: CYAN, a: 1 };
  }
  return { c: BG, a: 1 };
}

const SS = 2; // 超采样
const data = Buffer.alloc(W * H * 4);
for (let py = 0; py < H; py++) {
  for (let px = 0; px < W; px++) {
    let r = 0, g = 0, b = 0, a = 0;
    for (let sy = 0; sy < SS; sy++) {
      for (let sx = 0; sx < SS; sx++) {
        const s = sample((px + (sx + 0.5) / SS), (py + (sy + 0.5) / SS));
        r += s.c[0] * s.a; g += s.c[1] * s.a; b += s.c[2] * s.a; a += s.a;
      }
    }
    const n = SS * SS;
    const idx = (py * W + px) * 4;
    data[idx] = a ? Math.round(r / a) : 0;
    data[idx + 1] = a ? Math.round(g / a) : 0;
    data[idx + 2] = a ? Math.round(b / a) : 0;
    data[idx + 3] = Math.round((a / n) * 255);
  }
}

// ---- PNG 编码 ----
const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[i] = c >>> 0;
  }
  return t;
})();
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, d) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(d.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), d]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(W, 0);
ihdr.writeUInt32BE(H, 4);
ihdr[8] = 8; ihdr[9] = 6;
const stride = W * 4;
const raw = Buffer.alloc((stride + 1) * H);
for (let y = 0; y < H; y++) {
  raw[y * (stride + 1)] = 0;
  data.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
}
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);
writeFileSync(join(OUT, "banner-mark.png"), png);
console.log("banner mark written:", OUT, "banner-mark.png", `${W}x${H}`);
