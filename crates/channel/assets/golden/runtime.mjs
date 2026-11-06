// Metis 宿主运行时：被注入宿主的扩展/插件共用这一份。
//
// 设计约束：
// 1. **零依赖**：只用 Node 内置模块，不 import 任何第三方包；
// 2. **绝不抛进宿主**：所有入口都包 try/catch，读不到配置就当"没有配置"；
// 3. **只改自己写过的内容**：见 `Provenance`（有来源记录才还原，否则原样保留）；
// 4. **数据目录由生成器注入**：本文件不硬编码任何路径。
//
// 这里刻意不用任何构建期语言特性（不限 Node 版本），因为宿主的运行时版本我们控制不了。

import { readFileSync } from 'node:fs';

/** 生成器注入的常量。 */
export const constants = {
  home: null, // 数据目录（由生成器注入，例如 ~/.metis）
  harness: null, // 执行器 id
};

/**
 * 判断一行是不是"身份句"。
 *
 * 只认两种同时成立的：以第一人称身份开头（"You are …"、"你是 …"），且提到某个已知产品名。
 * 两个条件缺一不可——只靠其中一个会误删用户的普通规则。
 */
const SELF_START = /^\s*(?:You are|You're|你是|您是|我(?:是|现在扮演|就是))/i;
const PRODUCT_NAME =
  /(Claude|ChatGPT|GPT-?[45]|Gemini|Codex|Copilot|DeepSeek|Qwen|Kimi|GLM|Cursor|Windsurf|OpenCode|ZCode|CodeBuddy|Hermes|Grok|Antigravity|AI (?:assistant|model)|coding (?:assistant|agent)|语言模型|助手)/i;

export function stripIdentity(text) {
  const source = String(text ?? '');
  if (!source) return '';
  const kept = [];
  for (const line of source.split(/\r?\n/)) {
    if (SELF_START.test(line) && PRODUCT_NAME.test(line)) continue;
    kept.push(line);
  }
  return kept.join('\n').trim();
}

/**
 * 按模式重写一组宿主文本。
 *
 * - `off`：原样返回；
 * - `strict`：只留人设；
 * - `append`：宿主文本在前，人设追加在后；
 * - `replace`：人设在最前，宿主的身份句被剥掉。
 */
export function rewriteTexts(hostTexts, persona, mode) {
  const host = (Array.isArray(hostTexts) ? hostTexts : []).map((t) => String(t ?? '')).filter((t) => t.trim());
  if (mode === 'off' || !persona) return host;
  if (mode === 'strict') return [persona];
  if (mode === 'append') return [...host, persona];
  return [persona, ...host.map(stripIdentity).filter(Boolean)];
}

/**
 * 有来源记录的变换表。
 *
 * 为什么需要它：宿主会把我们改过的文本在下一轮**再交回来**。如果我们每次都"按样子"重写，
 * 就会越叠越多。这里记住 before → after 的对应关系，下一次先还原成 before 再重新算，
 * 因此**重复调用是幂等的**。
 *
 * 记录有上限（默认 64 条），且"同一个 after 对应过两个不同的 before"会标记为歧义 —— 歧义时
 * 一律原样返回，不猜。
 */
export function createProvenance(limit = 64) {
  const byKey = new Map();
  const byObject = new WeakMap();

  const keyOf = (value) => {
    try {
      return JSON.stringify(value);
    } catch {
      return null;
    }
  };

  function remember(before, after) {
    const beforeKey = keyOf(before);
    const afterKey = keyOf(after);
    if (beforeKey === null || afterKey === null || beforeKey === afterKey) return after;

    const entry = { before, beforeKey, afterKey };
    if (after && typeof after === 'object') byObject.set(after, entry);

    const previous = byKey.get(afterKey);
    if (previous && previous.beforeKey !== beforeKey) {
      byKey.set(afterKey, { ambiguous: true });
    } else if (!previous || !previous.ambiguous) {
      byKey.set(afterKey, entry);
    }

    while (byKey.size > limit) {
      const oldest = byKey.keys().next();
      if (oldest.done) break;
      byKey.delete(oldest.value);
    }
    return after;
  }

  function restore(value) {
    if (value && typeof value === 'object') {
      const direct = byObject.get(value);
      if (direct) return direct.before;
    }
    const key = keyOf(value);
    if (key === null) return value;
    const entry = byKey.get(key);
    if (!entry || entry.ambiguous) return value;
    return entry.before;
  }

  return { remember, restore };
}

/** 带来源记录的重写器：先还原、再重写、再记录。 */
export function createRewriter() {
  const provenance = createProvenance();
  return (hostTexts, persona, mode) => {
    const host = provenance.restore(hostTexts);
    if (mode === 'off' || !persona) return host;
    return provenance.remember(host, rewriteTexts(host, persona, mode));
  };
}

/** 读 `state.json`；任何异常都当成"没有配置"。 */
export function readState(statePath) {
  try {
    const parsed = JSON.parse(readFileSync(statePath, 'utf8'));
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null;
    return parsed;
  } catch {
    return null;
  }
}

/** 人设 id 的合法形状（与核心 crate 的规则一致）。 */
const PERSONA_ID = /^[a-z0-9][a-z0-9._-]{0,63}$/i;

/**
 * 读人设正文：剥掉 front-matter，只返回正文。
 *
 * 找不到就返回空串 —— 调用方据此判断"没有可用人设"，而不是用半份配置去注入。
 */
export function readPersona(personasDir, id) {
  if (!id || id === 'active' || !PERSONA_ID.test(id)) return '';
  try {
    const raw = readFileSync(`${personasDir}/${id}.md`, 'utf8');
    // front-matter 是 `---` 包起来的一段；没有就整份都是正文。
    const stripped = raw.replace(/^\uFEFF?---\r?\n[\s\S]*?\r?\n---(?:\r?\n|$)/, '');
    return stripped.trim();
  } catch {
    return '';
  }
}

/**
 * 解析激活：**与 Rust 侧同一套语义**（`crates/core/src/activation.rs` 的九步短路）。
 *
 * 这是"配置值"与"运行时许可"分离的实现：默认关闭，只有明确许可才注入。
 */
export function resolveActivation(state, harness, context = {}) {
  const STOP = (reason) => ({ on: false, mode: 'off', persona: null, blockedBy: reason, source: null });
  if (!state || typeof state !== 'object' || Array.isArray(state)) return STOP('missing-state');

  const override = (state.overrides && state.overrides[harness]) || {};
  const mode = override.mode || (state.default && state.default.mode) || 'replace';
  const persona = override.persona || state.persona || null;

  if (state.default && state.default.mode === 'off') return STOP('global-pause');
  if (override.mode === 'off') return STOP('harness-off');
  if (Array.isArray(state.disabledHarnesses) && state.disabledHarnesses.includes(harness)) return STOP('harness-disabled');
  if (context.session === false || context.enabled === false) return STOP('session-off');
  if (context.profile && Array.isArray(state.dshProfiles) && !state.dshProfiles.includes(context.profile)) return STOP('profile-off');
  if (!['strict', 'replace', 'append'].includes(mode)) return STOP(mode === 'off' ? 'mode-off' : 'invalid-mode');

  const channels = Array.isArray(state.armed && state.armed[harness]) ? state.armed[harness] : [];
  let source = null;
  if (context.launch === true) source = 'explicit-launch';
  else if (context.channel === 'install' && channels.includes('install')) source = 'resident';
  else if (context.channel === 'slash' && channels.includes('slash')) {
    if (context.session === true) source = 'session-command';
    else if (state[`${harness}Auto`] === true) source = 'auto';
  }
  if (!source) return STOP('not-enabled');

  return { on: true, mode, persona, blockedBy: null, source };
}

/** epoch 变了就说明中间被关过一次：旧的会话内覆盖必须失效。 */
export function epochOf(state, harness) {
  const global = (state && state.activationEpoch) || 'initial';
  const perHarness = (state && state.harnessEpochs && state.harnessEpochs[harness]) || 'initial';
  return `${global}:${perHarness}`;
}

/** 把 `/purge` 的参数字符串拆成 `[命令, 参数]`。 */
export function parseCommand(raw) {
  const parts = String(raw ?? '').trim().split(/\s+/);
  return { command: parts[0] || '', argument: parts[1] || '' };
}
