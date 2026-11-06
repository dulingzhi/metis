// Metis 托管插件（opencode v1 · `chat.system.transform`）。
//
// 由 Metis 写入；整份覆盖，不要手工编辑。
// 每轮读配置，所以换人设 / 改模式 / 暂停都在**下一轮**生效，不需要重装。
//
// 与 v2 的差别：v1 用 `chat.system.transform` 钩子、system 是**字符串数组**。

import { createRewriter, readPersona, readState, resolveActivation } from './runtime.mjs';

const HOME = "C:/Users/example/.metis";
const HARNESS = "codex";
const STATE = `${HOME}/state.json`;
const PERSONAS = `${HOME}/personas`;

/** 每个会话一个重写器，避免不同会话的来源记录互相污染。 */
const rewriters = new Map();

function rewriterFor(sessionId) {
  const key = sessionId || 'default';
  if (!rewriters.has(key)) rewriters.set(key, createRewriter());
  // 有界：会话多了不能无限长。
  while (rewriters.size > 64) {
    const oldest = rewriters.keys().next();
    if (oldest.done) break;
    rewriters.delete(oldest.value);
  }
  return rewriters.get(key);
}

function current() {
  const state = readState(STATE);
  if (!state) return null;
  const activation = resolveActivation(state, HARNESS, { channel: 'install' });
  if (!activation.on) return null;
  const body = readPersona(PERSONAS, activation.persona);
  if (!body) return null;
  return { mode: activation.mode, body };
}

export const Metis = async () => {
  const transform = async (input, output) => {
    try {
      if (!output || !Array.isArray(output.system)) return;
      const c = current();
      const next = rewriterFor(input?.sessionID)(output.system, c?.body || '', c?.mode || 'off');
      // 只在真的变了的时候原地替换，避免无意义的写。
      if (JSON.stringify(next) !== JSON.stringify(output.system)) {
        output.system.splice(0, output.system.length, ...next);
      }
    } catch {
      // 插件绝不能把宿主搞挂。
    }
  };
  return {
    'chat.system.transform': transform,
    'experimental.chat.system.transform': transform,
  };
};
