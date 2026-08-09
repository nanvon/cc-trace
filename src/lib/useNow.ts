/**
 * 共享当前时刻，按固定步长推进。
 *
 * 紧凑时长（`6d2h`）是按「距现在还有多久」算的，本身没有任何响应式输入：绝对时钟
 * 不需要重算，倒计时需要。没有它，读数会冻在渲染那一刻，要等下一次额度刷新
 * （15～60 分钟）才跳一次。
 *
 * 默认精度停在分钟：秒级跳字没有决策价值，又会让面板每秒重排一次。唯一例外是头部
 * 副标题「最近成功刷新」在刷新后 1 分钟窗口内需要秒级读数，因此另开一个 1 秒步长
 * 的 `useNowSeconds`，只服务那一处（见 ADR-0019 修订）；窗口外的消费方一律用分钟级。
 *
 * 每个步长一个计时器，全应用共享，不是每个组件一个；最后一个使用者离开时停表。
 */

import { onScopeDispose, readonly, ref, type Ref } from "vue";

const MINUTE_MS = 60_000;

function createTicker(stepMs: number): () => Readonly<Ref<Date>> {
  const current = ref(new Date());

  let timer: ReturnType<typeof setInterval> | null = null;
  let consumers = 0;

  function tick(): void {
    current.value = new Date();
  }

  /**
   * 隐藏期间即使继续走表，回到前台时也可能差了一个步长。
   * 重新可见时立刻校准一次，避免面板刚打开就显示上一拍的读数。
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
    timer = setInterval(tick, stepMs);
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

  return function useTick(): Readonly<Ref<Date>> {
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
  };
}

/** 分钟级推进，默认选择：倒计时、数据新鲜度等不需要秒级跳字的位置。 */
export const useNow = createTicker(MINUTE_MS);

/** 秒级推进，只给「最近成功刷新」副标题在 1 分钟窗口内使用。 */
export const useNowSeconds = createTicker(1_000);
