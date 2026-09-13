#!/usr/bin/env node
// Draws the SoundPush app icon (1024×1024 PNG) without dependencies.
// Usage: node tools/make-icon.mjs <out.png>
import { writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";

const size = 1024;
const out = process.argv[2] ?? "icon.png";
const px = Buffer.alloc(size * size * 4);

const accent = [0x28, 0x60, 0xe6];
const radius = 220;
// Five rounded bars, like a sound level.
const bars = [
  { x: 262, h: 220 },
  { x: 387, h: 420 },
  { x: 512, h: 600 },
  { x: 637, h: 420 },
  { x: 762, h: 220 },
];
const barW = 70;

function insideRoundedRect(x, y, x0, y0, x1, y1, r) {
  const cx = Math.min(Math.max(x, x0 + r), x1 - r);
  const cy = Math.min(Math.max(y, y0 + r), y1 - r);
  return (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
}

for (let y = 0; y < size; y++) {
  for (let x = 0; x < size; x++) {
    const i = (y * size + x) * 4;
    // 4×4 supersampling for smooth edges.
    let bg = 0;
    let fg = 0;
    for (let sy = 0; sy < 4; sy++) {
      for (let sx = 0; sx < 4; sx++) {
        const fx = x + (sx + 0.5) / 4;
        const fy = y + (sy + 0.5) / 4;
        if (insideRoundedRect(fx, fy, 40, 40, size - 40, size - 40, radius)) {
          bg++;
          if (bars.some((b) => insideRoundedRect(fx, fy, b.x - barW / 2, 512 - b.h / 2, b.x + barW / 2, 512 + b.h / 2, barW / 2))) fg++;
        }
      }
    }
    const a = bg / 16;
    const f = fg / 16;
    const mix = (c) => Math.round(c * (1 - f / Math.max(a, 1e-6)) + 255 * (f / Math.max(a, 1e-6)));
    px[i] = mix(accent[0]);
    px[i + 1] = mix(accent[1]);
    px[i + 2] = mix(accent[2]);
    px[i + 3] = Math.round(a * 255);
  }
}

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
};

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(size, 0);
ihdr.writeUInt32BE(size, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA
const raw = Buffer.alloc((size * 4 + 1) * size);
for (let y = 0; y < size; y++) {
  raw[y * (size * 4 + 1)] = 0;
  px.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
}
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);
writeFileSync(out, png);
console.log(`wrote ${out}`);
