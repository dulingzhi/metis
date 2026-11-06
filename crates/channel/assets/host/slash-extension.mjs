// Metis 托管扩展（pi / omp / pi-desktop）。
//
// 由 Metis 在安装 slash 通道时写入；**不要手工编辑**：Metis 会整份覆盖。
// 每轮读取配置，所以换人设、改模式、暂停都无需重装扩展。
//
// 注入点：`before_agent_start` —— 这是唯一能把内容放进 **system 提示词层**的钩子。

import { readFileSync } from 'node:fs';
import { createRewriter, epochOf, parseCommand, readPersona, readState, resolveActivation } from './runtime.mjs';

const HOME = __METIS_HOME__;
const HARNESS = __METIS_HARNESS__;
const STATE = `${HOME}/state.json`;
const PERSONAS = `${HOME}/personas`;

/** 会话内的覆盖：`null` = 跟随配置，`true/false` = 本次会话强制开关。 */
let sessionOverride = null;
/** 会话内临时切换的模式（`/purge mode …`）。 */
let sessionMode = null;
/** 覆盖生效时的 epoch；epoch 一变就作废。 */
let overrideEpoch = null;

const rewriter = createRewriter();

/** 当前该不该注入、注入什么。 */
function current() {
  const state = readState(STATE);
  if (!state) return { on: false, mode: 'off', body: '', persona: null, blockedBy: 'missing-state' };

  const epoch = epochOf(state, HARNESS);
  if (overrideEpoch && overrideEpoch !== epoch) {
    // 全局或本家被关过又打开：旧的会话内覆盖不该继续生效。
    sessionOverride = null;
    sessionMode = null;
    overrideEpoch = null;
  }

  const activation = resolveActivation(state, HARNESS, {
    channel: 'slash',
    session: sessionOverride === null ? undefined : sessionOverride,
    mode: sessionMode || undefined,
  });
  if (!activation.on) return { ...activation, body: '', state, epoch };

  const body = readPersona(PERSONAS, activation.persona);
  if (!body) return { ...activation, on: false, body: '', blockedBy: 'no-persona', state, epoch };
  return { ...activation, body, state, epoch };
}

export default function metis(api) {
  // 会话开始：丢掉上一轮的所有临时状态。
  api.on?.('session_start', () => {
    sessionOverride = null;
    sessionMode = null;
    overrideEpoch = null;
  });

  api.on?.('before_agent_start', (event) => {
    try {
      const c = current();
      const input = Array.isArray(event?.systemPrompt) ? event.systemPrompt : [event?.systemPrompt ?? ''];
      const next = rewriter(input, c.on ? c.body : '', c.on ? c.mode : 'off');
      // omp 要数组，pi / pi-desktop 要字符串 —— 这是宿主 API 的差异，不是我们的选择。
      return { systemPrompt: Array.isArray(event?.systemPrompt) ? next : next.join('\n\n') };
    } catch {
      // 扩展绝不能把宿主搞挂：出错就原样放行。
      return undefined;
    }
  });

  api.registerCommand?.('purge', {
    description: 'Metis：会话开关、人设与模式',
    getArgumentCompletions: (prefix) =>
      ['on', 'off', 'status', 'reload', 'mode'].filter((v) => v.startsWith(prefix)).map((value) => ({ value, label: value })),
    handler: async (args, ctx) => {
      const tell = (text, level = 'info') => {
        try {
          ctx?.ui?.notify?.(text, level);
        } catch {
          /* 宿主没有 notify 就算了 */
        }
      };
      try {
        const { command, argument } = parseCommand(args);
        const c = current();

        if (command === 'status' || command === 'reload') {
          const lines = [
            `状态: ${c.on ? '开启' : '关闭'}`,
            `模式: ${c.mode}`,
            `人设: ${c.persona || '未设置'}`,
            `来源: ${c.source || c.blockedBy || '无'}`,
            `配置: ${STATE}`,
          ];
          return tell(lines.join('\n'));
        }

        if (command === 'off' || (!command && c.on)) {
          sessionOverride = false;
          overrideEpoch = c.epoch;
          return tell('本次会话已关闭：下一轮不再注入');
        }

        if (command === 'on' || !command) {
          if (!c.body && !c.on) {
            const reason = c.blockedBy === 'missing-state' ? '读不到配置' : c.blockedBy || '未知原因';
            return tell(`当前无法开启：${reason}`, 'error');
          }
          sessionOverride = true;
          overrideEpoch = c.epoch;
          return tell('本次会话已开启：下一轮读取当前人设');
        }

        if (command === 'mode') {
          if (!['strict', 'replace', 'append'].includes(argument)) {
            return tell('用法: /purge mode strict|replace|append', 'error');
          }
          sessionMode = argument;
          overrideEpoch = c.epoch;
          return tell(`本次会话模式 → ${argument}（暂停与停用仍然优先）`);
        }

        return tell('用法: /purge [on|off|status|reload|mode strict|replace|append]', 'error');
      } catch (error) {
        return tell(String(error && error.message ? error.message : error), 'error');
      }
    },
  });
}
