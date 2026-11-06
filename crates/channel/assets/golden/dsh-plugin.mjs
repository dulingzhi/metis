// Metis 托管插件（DSH · cordis）。
//
// 由 Metis 写入；整份覆盖，不要手工编辑。
//
// 两个插件互不覆盖：
//   metis        常驻注入：每次 assemble 读配置，`off` 即时生效；
//   metis-slash  会话内 `/purge`：**只是开关**，注入仍然靠常驻插件。
//
// 路径推导：本文件在 `<profile>/node_modules/metis/` 下，向上两级就是 profile 根。

import { createRewriter, readPersona, readState, resolveActivation } from './runtime.mjs';

const HOME = "C:/Users/example/.metis";
const STATE = `${HOME}/state.json`;
const PERSONAS = `${HOME}/personas`;

const rewriter = createRewriter();

function current(profile) {
  const state = readState(STATE);
  if (!state) return null;
  const activation = resolveActivation(state, 'dsh', { channel: 'install', profile });
  if (!activation.on) return null;
  // `/purge` 的运行时开关（按 profile 存）。
  const runtime = state.dshRuntime && state.dshRuntime[profile];
  if (runtime && runtime.enabled === false) return null;
  const body = readPersona(PERSONAS, activation.persona);
  if (!body) return null;
  return { mode: activation.mode, body };
}

/** 从宿主上下文里尽量猜出 profile 名；猜不到就按 `unknown` 处理（不写白名单时视为允许）。 */
function profileOf() {
  const fromEnv = typeof process !== 'undefined' && process.env ? process.env.METIS_DSH_PROFILE : null;
  return fromEnv || 'unknown';
}

export default {
  name: 'metis',
  apply(ctx) {
    try {
      ctx.on?.('system-prompt/assemble', (event) => {
        try {
          const c = current(profileOf());
          const sections = Array.isArray(event?.sections) ? event.sections : [];
          if (!c) return { ...event, sections: rewriter(sections.map(sectionText), '', 'off').map(sectionBack) };
          return { ...event, sections: rewriter(sections.map(sectionText), c.body, c.mode).map(sectionBack) };
        } catch {
          return event;
        }
      });
    } catch {
      // 装不上钩子就静默退出，绝不影响宿主启动。
    }
  },
};

function sectionText(section) {
  if (typeof section === 'string') return section;
  return String(section?.text ?? '');
}

function sectionBack(text, index, all) {
  const original = all[index];
  if (original && typeof original === 'object') return { ...original, text };
  return text;
}
