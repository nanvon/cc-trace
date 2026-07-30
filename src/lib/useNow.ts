/**
 * 每分钟推进一次的共享当前时刻。
 *
 * 紧凑时长（`6d2h`）是按「距现在还有多久」算的，本身没有任何响应式输入：绝对时钟
 * 不需要重算，倒计时需要。没有它，读数会冻在渲染那一刻，要等下一次额度刷新
 * （15～60 分钟）才跳一次。
 *
 * 精度刻意停在分钟：秒级跳字既没有决策价值，又会让面板每秒重排一次。
 *
 * 全应用共用一个计时器，不是每个组件一个；最后一个使用者离开时停表。
 */

import { onScopeDispose, readonly, ref, type Ref } from "vue";

const TICK_MS = 60_000;

const current = ref(new Date());

let timer: ReturnType<typeof setInterval> | null = null;
let consumers = 0;

function tick(): void {
  current.value = new Date();
}

/**
 * 隐藏期间即使继续走表，回到前台时也可能差了不到一分钟。
 * 重新可见时立刻校准一次，避免面板刚打开就显示上一分钟的读数。
 */
function handleVisibility(): void {
  if (document.visibilityState === "visible") {
    tick();
  }
}

function start(): void {
  if (timer !== null) {
    return;
  }
  timer = setInterval(tick, TICK_MS);
  document.addEventListener("visibilitychange", handleVisibility);
}

function stop(): void {
  if (timer === null) {
    return;
  }
  clearInterval(timer);
  timer = null;
  document.removeEventListener("visibilitychange", handleVisibility);
}

export function useNow(): Readonly<Ref<Date>> {
  consumers += 1;
  tick();
  start();

  onScopeDispose(() => {
    consumers -= 1;
    if (consumers <= 0) {
      consumers = 0;
      stop();
    }
  });

  return readonly(current);
}
