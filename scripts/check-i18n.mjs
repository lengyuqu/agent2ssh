// Temporary dev script: list t("...") keys used in src/ but missing from the
// zh translation map in src/i18n.tsx. Run: node scripts/check-i18n.mjs
import fs from "node:fs";
import path from "node:path";

const zhSrc = fs.readFileSync("src/i18n.tsx", "utf8");
const objStart = zhSrc.indexOf("const zh:");
const objEnd = zhSrc.indexOf("const I18nContext");
const block = zhSrc.slice(objStart, objEnd);
const zh = new Set();
const keyRe = /^\s*"((?:[^"\\]|\\.)*)"\s*:/gm;
let m;
while ((m = keyRe.exec(block))) zh.add(m[1]);

function walk(dir) {
  for (const f of fs.readdirSync(dir)) {
    const p = path.join(dir, f);
    const st = fs.statSync(p);
    if (st.isDirectory()) {
      walk(p);
    } else if ((f.endsWith(".tsx") || f.endsWith(".ts")) && !f.includes(".test.")) {
      const src = fs.readFileSync(p, "utf8");
      const callRe = /(?<![\w.$])t\(\s*"((?:[^"\\]|\\.)*)"/g;
      let mm;
      while ((mm = callRe.exec(src))) {
        if (!zh.has(mm[1])) console.log(`${p}: ${mm[1]}`);
      }
    }
  }
}

walk("src");
