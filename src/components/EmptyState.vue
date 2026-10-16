<script setup>
defineProps({ text: String });
</script>

<template>
  <div class="empty">
    <div class="icon">
      <svg width="40" height="40" viewBox="0 0 40 40" fill="none">
        <rect x="6" y="10" width="28" height="24" rx="4" stroke="#7b8694" stroke-width="2" />
        <path d="M14 10 V6 a3 3 0 0 1 3-3 h6 a3 3 0 0 1 3 3 v4" stroke="#7b8694" stroke-width="2" />
        <path d="M14 20 h12 M14 26 h8" stroke="#7b8694" stroke-width="2" stroke-linecap="round" />
      </svg>
    </div>
    <div>{{ text }}</div>
    <div class="act"><slot /></div>
  </div>
</template>
