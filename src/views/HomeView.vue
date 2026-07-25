<script setup lang="ts">
import { storeToRefs } from "pinia";
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";

import brandSymbol from "../assets/brand/cc-trace-symbol.svg";
import { useAppStore } from "../stores/app";

const { t } = useI18n();
const appStore = useAppStore();
const { status } = storeToRefs(appStore);
const runtimeLabel = computed(() =>
  status.value
    ? `${status.value.name} ${status.value.version} · ${status.value.platform}`
    : t("foundation.stack"),
);

onMounted(() => {
  void appStore.loadStatus();
});
</script>

<template>
  <main class="foundation">
    <section class="foundation__identity" aria-labelledby="foundation-title">
      <img
        class="foundation__mark"
        :src="brandSymbol"
        alt=""
        aria-hidden="true"
      />

      <div class="foundation__copy">
        <p class="foundation__eyebrow">{{ t("foundation.eyebrow") }}</p>
        <h1 id="foundation-title">{{ t("foundation.title") }}</h1>
        <p class="foundation__description">
          {{ t("foundation.description") }}
        </p>
      </div>
    </section>

    <section class="foundation__status" aria-labelledby="status-title">
      <h2 id="status-title">{{ t("foundation.statusTitle") }}</h2>
      <dl>
        <div>
          <dt>Stack</dt>
          <dd>{{ runtimeLabel }}</dd>
        </div>
        <div>
          <dt>Scope</dt>
          <dd>{{ t("foundation.scope") }}</dd>
        </div>
        <div>
          <dt>Boundary</dt>
          <dd>{{ t("foundation.boundary") }}</dd>
        </div>
      </dl>
    </section>
  </main>
</template>

<style scoped>
.foundation {
  min-height: 100%;
  display: grid;
  grid-template-columns: minmax(0, 1.35fr) minmax(18rem, 0.65fr);
}

.foundation__identity {
  position: relative;
  min-height: 100%;
  display: flex;
  align-items: flex-end;
  overflow: hidden;
  padding: clamp(3rem, 8vw, 7.5rem);
  background: var(--surface-primary);
}

.foundation__mark {
  position: absolute;
  width: min(48rem, 78vw);
  right: -17%;
  top: 8%;
  opacity: 0.055;
  transform: rotate(-8deg);
  pointer-events: none;
  user-select: none;
}

.foundation__copy {
  position: relative;
  max-width: 44rem;
}

.foundation__eyebrow,
.foundation__status dt {
  font-family: var(--font-utility);
  font-size: 0.75rem;
  letter-spacing: 0.12em;
  text-transform: uppercase;
}

.foundation__eyebrow {
  margin: 0 0 1.25rem;
  color: var(--text-tertiary);
}

h1 {
  max-width: 10ch;
  margin: 0;
  font-size: clamp(3rem, 7vw, 6.75rem);
  font-weight: 650;
  letter-spacing: -0.065em;
  line-height: 0.92;
}

.foundation__description {
  max-width: 38rem;
  margin: 2rem 0 0;
  color: var(--text-secondary);
  font-size: clamp(1rem, 1.4vw, 1.2rem);
  line-height: 1.75;
}

.foundation__status {
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  padding: clamp(2rem, 5vw, 4rem);
  color: var(--text-on-dark);
  background: var(--surface-inverse);
}

.foundation__status h2 {
  margin: 0 0 2.5rem;
  font-size: 0.8rem;
  font-weight: 560;
  letter-spacing: 0.12em;
  text-transform: uppercase;
}

.foundation__status dl {
  margin: 0;
}

.foundation__status dl div {
  padding: 1.4rem 0;
  border-top: 1px solid var(--border-on-dark);
}

.foundation__status dt {
  margin-bottom: 0.65rem;
  color: var(--text-muted-on-dark);
}

.foundation__status dd {
  margin: 0;
  line-height: 1.55;
}

@media (max-width: 760px) {
  .foundation {
    grid-template-columns: 1fr;
  }

  .foundation__identity {
    min-height: 62vh;
  }
}

@media (prefers-color-scheme: dark) {
  .foundation__mark {
    filter: invert(1);
  }
}

@media (prefers-reduced-motion: reduce) {
  .foundation__mark {
    transform: none;
  }
}
</style>
