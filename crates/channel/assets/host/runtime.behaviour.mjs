/**
 * 宿主运行时的行为测试（由 `cargo test` 通过 node 调用）。
 *
 * 为什么需要它：`slash` 通道的产物是**宿主真正会执行的 JavaScript**。
 * 只做 `node --check` 语法检查是不够的 —— 语法对但语义错，用户的提示词就会出错。
 * 这里直接跑一遍真实的运行时函数，断言注入、模式、幂等与失败降级。
 *
 * 输入：`METIS_RUNTIME`（运行时文件路径）、`METIS_TEST_HOME`（临时数据目录）。
 * 输出：一行 `PASS n` 或非零退出 + 失败原因。
 */

import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';

const runtimePath = process.env.METIS_RUNTIME;
const testHome = process.env.METIS_TEST_HOME;
if (!runtimePath || !testHome) {
  console.error('缺少 METIS_RUNTIME 或 METIS_TEST_HOME');
  process.exit(2);
}

mkdirSync(`${testHome}/personas`, { recursive: true });
// 用动态 import 加载真实产物（不是复制一份）。
const rt = await import(`file://${runtimePath.replace(/\\/g, '/')}`);

let passed = 0;
const check = (label, condition, detail) => {
  if (!condition) {
    console.error(`✗ ${label}${detail ? `\n  ${detail}` : ''}`);
    process.exit(1);
  }
  passed += 1;
};

const writeState = (state) => writeFileSync(`${testHome}/state.json`, JSON.stringify(state));
const writePersona = (id, body) => writeFileSync(`${testHome}/personas/${id}.md`, `---\nid: ${id}\n---\n\n${body}\n`);

// ── stripIdentity ──────────────────────────────────────────────────────────
check(
  'stripIdentity 删掉身份句',
  rt.stripIdentity('You are a Claude agent.\nKeep this rule.') === 'Keep this rule.',
  rt.stripIdentity('You are a Claude agent.\nKeep this rule.'),
);
check('stripIdentity 不误删普通规则', rt.stripIdentity('You are welcome to edit files.') === 'You are welcome to edit files.');
check('stripIdentity 不误删中文普通句', rt.stripIdentity('我是在改配置，不是在问问题。') === '我是在改配置，不是在问问题。');
check('stripIdentity 处理空输入', rt.stripIdentity('') === '');
check('stripIdentity 处理 null', rt.stripIdentity(null) === '');

// ── rewriteTexts 四种模式 ─────────────────────────────────────────────────
const host = ['You are a Claude agent.', 'Rule A'];
check('off 原样返回', JSON.stringify(rt.rewriteTexts(host, 'P', 'off')) === JSON.stringify(host));
check('strict 只留人设', JSON.stringify(rt.rewriteTexts(host, 'P', 'strict')) === JSON.stringify(['P']));
check('append 追加在后', JSON.stringify(rt.rewriteTexts(host, 'P', 'append')) === JSON.stringify([...host, 'P']));
check(
  'replace 剥掉身份句后前置人设',
  JSON.stringify(rt.rewriteTexts(host, 'P', 'replace')) === JSON.stringify(['P', 'Rule A']),
  JSON.stringify(rt.rewriteTexts(host, 'P', 'replace')),
);
check('没有人设时不注入', JSON.stringify(rt.rewriteTexts(host, '', 'replace')) === JSON.stringify(host));

// ── resolveActivation 九步短路 ────────────────────────────────────────────
const base = { default: { mode: 'replace' }, persona: 'concise', armed: { pi: ['slash'] } };
check('全局暂停压过一切', rt.resolveActivation({ ...base, default: { mode: 'off' } }, 'pi', { launch: true }).blockedBy === 'global-pause');
check('单家 off', rt.resolveActivation({ ...base, overrides: { pi: { mode: 'off' } } }, 'pi', { launch: true }).blockedBy === 'harness-off');
check(
  '整家停用',
  rt.resolveActivation({ ...base, disabledHarnesses: ['pi'] }, 'pi', { launch: true }).blockedBy === 'harness-disabled',
);
check('会话关闭', rt.resolveActivation(base, 'pi', { channel: 'slash', session: false }).blockedBy === 'session-off');
check(
  'profile 白名单',
  rt.resolveActivation({ ...base, dshProfiles: ['desktop'] }, 'dsh', { channel: 'install', profile: 'web' }).blockedBy === 'profile-off',
);
check('未武装', rt.resolveActivation({ ...base, armed: {} }, 'pi', { channel: 'slash' }).blockedBy === 'not-enabled');
check('显式启动授予许可', rt.resolveActivation(base, 'pi', { launch: true }).source === 'explicit-launch');
check(
  '会话内命令授予许可',
  rt.resolveActivation(base, 'pi', { channel: 'slash', session: true }).source === 'session-command',
);
check(
  'Auto 授予许可',
  rt.resolveActivation({ ...base, piAuto: true }, 'pi', { channel: 'slash' }).source === 'auto',
);
check('只给 channel 不给 session 不授予', rt.resolveActivation(base, 'pi', { channel: 'slash' }).on === false);
check('缺失状态', rt.resolveActivation(null, 'pi', { launch: true }).blockedBy === 'missing-state');
check('非法模式', rt.resolveActivation({ ...base, default: { mode: 'bogus' } }, 'pi', { launch: true }).blockedBy === 'invalid-mode');

// ── 人设读取 ──────────────────────────────────────────────────────────────
writePersona('concise', '用简体中文。');
check('读人设剥掉 front-matter', rt.readPersona(`${testHome}/personas`, 'concise') === '用简体中文。');
check('保留字 active 不读文件', rt.readPersona(`${testHome}/personas`, 'active') === '');
check('路径穿越被拒绝', rt.readPersona(`${testHome}/personas`, '../secret') === '');
check('不存在的人设返回空串', rt.readPersona(`${testHome}/personas`, 'ghost') === '');

// ── epoch ─────────────────────────────────────────────────────────────────
check('缺省 epoch 是 initial', rt.epochOf({}, 'pi') === 'initial:initial');
check('epoch 随字段变化', rt.epochOf({ activationEpoch: 'a', harnessEpochs: { pi: 'b' } }, 'pi') === 'a:b');

// ── createRewriter 幂等（最关键的一条） ──────────────────────────────────
const rewriter = rt.createRewriter();
const once = rewriter(host, 'P', 'replace');
const twice = rewriter(once, 'P', 'replace');
check('重复调用幂等', JSON.stringify(once) === JSON.stringify(twice), `${JSON.stringify(once)} vs ${JSON.stringify(twice)}`);
const back = rewriter(twice, '', 'off');
check('关掉之后还原成宿主原文', JSON.stringify(back) === JSON.stringify(host), JSON.stringify(back));

// ── readState 的失败降级 ──────────────────────────────────────────────────
check('状态文件不存在返回 null', rt.readState(`${testHome}/nope.json`) === null);
writeFileSync(`${testHome}/broken.json`, '{ not json');
check('状态文件损坏返回 null（不抛）', rt.readState(`${testHome}/broken.json`) === null);

// ── parseCommand ──────────────────────────────────────────────────────────
check('解析子命令', rt.parseCommand(' mode append ').argument === 'append');
check('解析空参数', rt.parseCommand('').command === '');

console.log(`PASS ${passed}`);
