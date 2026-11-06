// Metis 托管插件（opencode v2 · `ctx.session.hook("context")`）。
//
// 由 Metis 写入；整份覆盖，不要手工编辑。
// 每轮读配置，所以换人设 / 改模式 / 暂停都在**下一轮**生效，不需要重装。
//
// 与 v1 的差别：v2 走 session hook，system 是**内容块数组**（要保留非文本块与缓存元数据）。

import { createRewriter, readPersona, readState, resolveActivation } from './runtime.mjs';

const HOME = __METIS_HOME__;
const HARNESS = __METIS_HARNESS__;
const STATE = `${HOME}/state.json`;
const PERSONAS = `${HOME}/personas`;

const rewriters = new Map();

function rewriterFor(sessionId) {
  const key = sessionId || 'default';
  if (!rewriters.has(key)) rewriters.set(key, createRewriter());
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

export default {
  id: 'metis',
  async setup(ctx) {
    await ctx.session.hook('context', (event) => {
      try {
        if (!event || !Array.isArray(event.system)) return;
        const c = current();
        const rewriter = rewriterFor(event.sessionID ?? event.session?.id);
        // 内容块数组：先抽成纯文本参与比较，再按块的形状写回。
        const texts = event.system.map((block) => (typeof block === 'string' ? block : String(block?.text ?? '')));
        const next = rewriter(texts, c?.body || '', c?.mode || 'off');
        if (JSON.stringify(next) === JSON.stringify(texts)) return;
        const rebuilt = next.map((text, index) => {
          const original = event.system[index];
          return original && typeof original === 'object' ? { ...original, text } : text;
        });
        event.system.splice(0, event.system.length, ...rebuilt);
      } catch {
        // 插件绝不能把宿主搞挂。
      }
    });
  },
};
