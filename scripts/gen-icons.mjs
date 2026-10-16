// OpenFrpc 图标生成器：纯 Node 光栅化（无第三方依赖）
// 图案：「开放的隧道」——深板岩圆角方底 · 青色环在右侧留缺口 · 白色箭头穿环由缺口而出
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const OUT = join(process.cwd(), "src-tauri", "icons");
mkdirSync(OUT, { recursive: true });

// ---- 调色 ----
const BG = [24, 38, 54];      // #182636 深板岩蓝
const RING = [111, 211, 238]; // #6FD3EE 青色
const ARROW = [255, 255, 255];

// ---- 几何（坐标归一化到 [0,1]） ----
function inRoundedRect(x, y, cx, cy, half, r) {
  const qx = Math.abs(x - cx) - (half - r);
  const qy = Math.abs(y - cy) - (half - r);
  const dx = Math.max(qx, 0);
  const dy = Math.max(qy, 0);
  return Math.hypot(dx, dy) + Math.min(Math.max(qx, qy), 0) - r < 0;
}
// 开口环：缺口朝右（±36°），穿过缺口的箭头寓意「Open」
function inOpenRing(x, y, cx, cy, R, w) {
  const d = Math.hypot(x - cx, y - cy);
  if (Math.abs(d - R) >= w) return false;
  const ang = Math.atan2(y - cy, x - cx); // -π..π，0 朝右
  const deg = (ang * 180) / Math.PI;
  return !(deg > -36 && deg < 36);
}
function inShaft(x, y) {
  return x > 0.03 && x < 0.755 && Math.abs(y - 0.5) < 0.054;
}
function inHead(x, y) {
  const tipX = 0.93, baseX = 0.72, topY = 0.5, halfH = 0.165;
  if (x < baseX || x > tipX) return false;
  const t = (x - baseX) / (tipX - baseX);
  return Math.abs(y - topY) < halfH * (1 - t);
}

function sample(x, y) {
  if (!inRoundedRect(x, y, 0.5, 0.5, 0.5, 0.22)) return null;
  let c = BG;
  if (inOpenRing(x, y, 0.5, 0.5, 0.3, 0.088)) c = RING;
  if (inShaft(x, y) || inHead(x, y)) c = ARROW;
  return c;
}

function render(size) {
  const SS = 4; // 超采样
  const data = Buffer.alloc(size * size * 4);
  for (let py = 0; py < size; py++) {
    for (let px = 0; px < size; px++) {
      let r = 0, g = 0, b = 0, a = 0;
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const x = (px + (sx + 0.5) / SS) / size;
          const y = (py + (sy + 0.5) / SS) / size;
          const c = sample(x, y);
          if (c) { r += c[0]; g += c[1]; b += c[2]; a += 255; }
        }
      }
      const n = SS * SS;
      const idx = (py * size + px) * 4;
      data[idx] = a ? Math.round(r / (a / 255)) : 0;
      data[idx + 1] = a ? Math.round(g / (a / 255)) : 0;
      data[idx + 2] = a ? Math.round(b / (a / 255)) : 0;
      data[idx + 3] = Math.round(a / n);
    }
  }
  return data;
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
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}
function encodePNG(size, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6; // 8bit RGBA
  const stride = size * 4;
  const raw = Buffer.alloc((stride + 1) * size);
  for (let y = 0; y < size; y++) {
    raw[y * (stride + 1)] = 0; // filter none
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

// ---- ICO（PNG 条目） ----
function encodeICO(entries) {
  // entries: [{size, png}]
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(entries.length, 4);
  const dirs = [];
  const blobs = [];
  let offset = 6 + entries.length * 16;
  for (const e of entries) {
    const d = Buffer.alloc(16);
    d[0] = e.size >= 256 ? 0 : e.size;
    d[1] = e.size >= 256 ? 0 : e.size;
    d.writeUInt16LE(1, 4);   // planes
    d.writeUInt16LE(32, 6);  // bpp
    d.writeUInt32LE(e.png.length, 8);
    d.writeUInt32LE(offset, 12);
    dirs.push(d);
    blobs.push(e.png);
    offset += e.png.length;
  }
  return Buffer.concat([header, ...dirs, ...blobs]);
}

// ---- 产出 ----
const cache = new Map();
function pngOf(size) {
  if (!cache.has(size)) cache.set(size, encodePNG(size, render(size)));
  return cache.get(size);
}

const icoSizes = [16, 24, 32, 48, 64, 128, 256];
writeFileSync(join(OUT, "icon.ico"), encodeICO(icoSizes.map((s) => ({ size: s, png: pngOf(s) }))));
writeFileSync(join(OUT, "icon.png"), pngOf(1024));
writeFileSync(join(OUT, "32x32.png"), pngOf(32));
writeFileSync(join(OUT, "128x128.png"), pngOf(128));
writeFileSync(join(OUT, "128x128@2x.png"), pngOf(256));
console.log("icons written to", OUT, "sizes:", icoSizes.join(","), "+1024");
