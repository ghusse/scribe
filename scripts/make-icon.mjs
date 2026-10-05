import { deflateSync, crc32 } from "node:zlib";
import { writeFileSync } from "node:fs";

const size = 1024;
const raw = Buffer.alloc((size * 4 + 1) * size);
for (let y = 0; y < size; y++) {
  const row = y * (size * 4 + 1);
  raw[row] = 0;
  for (let x = 0; x < size; x++) {
    const dx = x - size / 2, dy = y - size / 2;
    const inside = Math.hypot(dx, dy) < size * 0.46;
    const mic =
      (Math.abs(dx) < size * 0.09 && dy > -size * 0.25 && dy < size * 0.12) ||
      (Math.abs(dx) < size * 0.018 && dy >= size * 0.12 && dy < size * 0.26);
    const i = row + 1 + x * 4;
    if (!inside) { raw[i + 3] = 0; continue; }
    const [r, g, b] = mic ? [255, 255, 255] : [79, 70, 229];
    raw[i] = r; raw[i + 1] = g; raw[i + 2] = b; raw[i + 3] = 255;
  }
}
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td) >>> 0);
  return Buffer.concat([len, td, crc]);
}
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(size, 0); ihdr.writeUInt32BE(size, 4);
ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
writeFileSync("app-icon.png", Buffer.concat([
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
  chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0)),
]));
