<script setup>
import { watch, onBeforeUnmount } from "vue";

const props = defineProps({
  title: String,
  modelValue: Boolean, // 是否显示
  wide: Boolean,
});
const emit = defineEmits(["close"]);

function onKey(e) {
  if (e.key === "Escape") emit("close");
}
watch(
  () => props.modelValue,
  (open) => {
    if (open) window.addEventListener("keydown", onKey);
    else window.removeEventListener("keydown", onKey);
  },
  { immediate: true }
);
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <Teleport to="body">
    <div v-if="modelValue" class="modal-mask" @click.self="emit('close')">
      <div class="modal" :style="wide ? 'width:min(860px,calc(100vw - 80px))' : ''">
        <div class="modal-head">
          <h3>{{ title }}</h3>
          <button class="modal-close" @click="emit('close')">✕</button>
        </div>
        <div class="modal-body">
          <slot />
        </div>
        <div class="modal-foot">
          <slot name="foot" />
        </div>
      </div>
    </div>
  </Teleport>
</template>
