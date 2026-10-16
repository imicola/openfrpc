<script setup>
defineProps({ on: Boolean, disabled: Boolean });
const emit = defineEmits(["toggle"]);
</script>

<template>
  <button class="switch" :class="{ on }" :disabled="disabled" @click="emit('toggle')" role="switch" :aria-checked="on"></button>
</template>
