<script setup lang="ts" generic="T extends string">
/**
 * 轻量下拉菜单（对齐 cc-bar 系统 Menu 形态）。
 *
 * trigger 显示当前值；菜单为悬浮层（--shadow-panel），支持点击外部关闭、
 * Esc／Tab 关闭、方向键导航、Enter 选中；选项可带右对齐计数与选中勾。
 * 键盘遵循 listbox 模式：菜单聚焦 + aria-activedescendant。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

export interface MenuSelectOption<T extends string = string> {
  value: T;
  label: string;
  count?: number;
}

const props = defineProps<{
  modelValue: T | null;
  options: MenuSelectOption<T>[];
  /** trigger 与菜单的可访问名称。 */
  label: string;
  /** 无选中值时的显示文案。 */
  emptyLabel?: string;
}>();

const emit = defineEmits<{
  (event: "update:modelValue", value: T | null): void;
}>();

const open = ref(false);
const activeIndex = ref(-1);
const root = ref<HTMLElement | null>(null);
const list = ref<HTMLElement | null>(null);

const selectedLabel = computed(() => {
  const hit = props.options.find((option) => option.value === props.modelValue);
  return hit?.label ?? props.emptyLabel ?? "";
});

function optionId(index: number): string {
  return `menu-select-option-${index}`;
}

function toggle(): void {
  open.value = !open.value;
  if (open.value) {
    const index = props.options.findIndex((option) => option.value === props.modelValue);
    activeIndex.value = Math.max(0, index);
    void nextTick(() => list.value?.focus());
  } else {
    activeIndex.value = -1;
  }
}

function choose(value: T | null): void {
  emit("update:modelValue", value);
  open.value = false;
  activeIndex.value = -1;
  root.value?.querySelector<HTMLElement>(".menu-select__trigger")?.focus();
}

function onTriggerKeydown(event: KeyboardEvent): void {
  if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    if (!open.value) toggle();
  } else if (event.key === "Escape" && open.value) {
    open.value = false;
    activeIndex.value = -1;
  }
}

function onListKeydown(event: KeyboardEvent): void {
  const last = props.options.length - 1;
  if (event.key === "ArrowDown") {
    event.preventDefault();
    activeIndex.value = activeIndex.value >= last ? 0 : activeIndex.value + 1;
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    activeIndex.value = activeIndex.value <= 0 ? last : activeIndex.value - 1;
  } else if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    if (activeIndex.value >= 0) choose(props.options[activeIndex.value].value);
  } else if (event.key === "Escape") {
    event.preventDefault();
    open.value = false;
    activeIndex.value = -1;
    root.value?.querySelector<HTMLElement>(".menu-select__trigger")?.focus();
  } else if (event.key === "Tab") {
    open.value = false;
    activeIndex.value = -1;
  }
}

function onDocumentPointerDown(event: PointerEvent): void {
  if (open.value && root.value && !root.value.contains(event.target as Node)) {
    open.value = false;
    activeIndex.value = -1;
  }
}

watch(activeIndex, () => {
  const el = list.value?.querySelector<HTMLElement>(`[data-index="${activeIndex.value}"]`);
  el?.scrollIntoView({ block: "nearest" });
});

onMounted(() => {
  document.addEventListener("pointerdown", onDocumentPointerDown, true);
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocumentPointerDown, true);
});
</script>

<template>
  <div ref="root" class="menu-select">
    <button
      type="button"
      class="menu-select__trigger"
      :aria-label="label"
      :aria-haspopup="'listbox'"
      :aria-expanded="open"
      @click="toggle"
      @keydown="onTriggerKeydown"
    >
      <span class="menu-select__label">{{ selectedLabel }}</span>
      <span class="menu-select__chevron" aria-hidden="true"></span>
    </button>

    <div
      v-if="open"
      ref="list"
      class="menu-select__list"
      role="listbox"
      :aria-label="label"
      :aria-activedescendant="activeIndex >= 0 ? optionId(activeIndex) : undefined"
      tabindex="-1"
      @keydown="onListKeydown"
    >
      <div
        v-for="(option, index) in options"
        :id="optionId(index)"
        :key="option.value"
        class="menu-select__option"
        :class="{ active: index === activeIndex }"
        role="option"
        :aria-selected="option.value === modelValue"
        :data-index="index"
        @click="choose(option.value)"
        @mousemove="activeIndex = index"
      >
        <span class="menu-select__option-label">{{ option.label }}</span>
        <span v-if="option.count !== undefined" class="menu-select__option-count numeric">
          {{ option.count }}
        </span>
        <span v-if="option.value === modelValue" class="menu-select__check" aria-hidden="true">
          ✓
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.menu-select {
  position: relative;
  min-inline-size: 0;
}

.menu-select__trigger {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  min-block-size: 2.25rem;
  max-inline-size: 12rem;
  padding: 0 0.75rem;
  border: 1px solid var(--border-subtle);
  border-radius: 0.5625rem;
  color: var(--text-primary);
  background: var(--surface-raised);
  font-size: 0.75rem;
  text-align: start;
  white-space: nowrap;
  cursor: pointer;
}

.menu-select__label {
  overflow: hidden;
  text-overflow: ellipsis;
}

.menu-select__chevron {
  inline-size: 0.375rem;
  block-size: 0.375rem;
  flex: none;
  border-inline-end: 1.5px solid var(--text-secondary);
  border-block-end: 1.5px solid var(--text-secondary);
  transform: rotate(45deg);
}

.menu-select__trigger[aria-expanded="true"] .menu-select__chevron {
  transform: rotate(-135deg);
}

@media (prefers-reduced-motion: no-preference) {
  .menu-select__chevron {
    transition: transform var(--motion-fast) var(--ease-out);
  }
}

/* 悬浮层：与状态点 tooltip 同一套材料（面板阴影 + 描边） */
.menu-select__list {
  position: absolute;
  inset-block-start: calc(100% + 4px);
  inset-inline-start: 0;
  z-index: 30;
  inline-size: max-content;
  min-inline-size: 100%;
  max-inline-size: 16rem;
  max-block-size: 18rem;
  overflow-y: auto;
  padding: 0.25rem;
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
  box-shadow: var(--shadow-panel);
  outline: none;
}

.menu-select__option {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-block-size: 2rem;
  padding: 0 0.5rem;
  border-radius: 0.5rem;
  color: var(--text-primary);
  font-size: 0.75rem;
  cursor: pointer;
}

.menu-select__option.active {
  background: var(--action-soft);
}

.menu-select__option-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.menu-select__option-count {
  margin-inline-start: auto;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.menu-select__check {
  flex: none;
  color: var(--action-primary);
  font-size: 0.6875rem;
}
</style>
