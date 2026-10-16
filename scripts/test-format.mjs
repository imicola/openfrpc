// parsePortSpec 回归测试（R02）：node scripts/test-format.mjs
import assert from "node:assert";
import { parsePortSpec } from "../src/lib/format.js";

// 常规格式
let r = parsePortSpec("80,443,8000-8010");
assert.deepStrictEqual(r, [80, 443, ...Array.from({ length: 11 }, (_, i) => 8000 + i)]);

// 超出安全整数范围的区间：必须立即返回空，不允许卡死主线程
let t0 = Date.now();
r = parsePortSpec("9007199254740992-9007199254740994");
assert.ok(Date.now() - t0 < 500, "超大区间应在 500ms 内返回");
assert.deepStrictEqual(r, []);

// 反向区间自动交换
r = parsePortSpec("8010-8000");
assert.strictEqual(r.length, 11);
assert.strictEqual(r[0], 8000);
assert.strictEqual(r[10], 8010);

// 区间裁剪到 1..65535
r = parsePortSpec("1-70000");
assert.strictEqual(r.length, 65535);
r = parsePortSpec("0-10");
assert.strictEqual(r[0], 1);
assert.strictEqual(r.length, 10);

// 单个超长数字直接跳过
r = parsePortSpec("99999999999999999999");
assert.deepStrictEqual(r, []);

// 非法片段跳过，不影响其他片段
r = parsePortSpec("abc, 80, x-y, 443");
assert.deepStrictEqual(r, [80, 443]);

// cap：超出上限时以 cap+1 个元素向调用方示意溢出
r = parsePortSpec("1-65535", 100);
assert.strictEqual(r.length, 101);

console.log("format tests: all passed");
