<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  function logPointerDown(event: PointerEvent) {
    console.log('[Re-Clippy] pointer down on Clippy', {
      button: event.button,
      x: event.clientX,
      y: event.clientY
    });
  }

  async function openChat() {
    console.log('[Re-Clippy] click on Clippy; requesting chat window');

    try {
      const status = await invoke<string>('open_chat');
      console.log(`[Re-Clippy] ${status}`);
    } catch (error) {
      console.error('[Re-Clippy] could not open chat window', error);
    }
  }
</script>

<svelte:head>
  <title>Re-Clippy</title>
</svelte:head>

<svelte:window oncontextmenu={(event) => event.preventDefault()} />

<main class="clippy-window">
  <button class="clippy-button" onpointerdown={logPointerDown} onclick={openChat} aria-label="Open Clippy chat">
    <img class="clippy-image" src="/clippy.webp" alt="Clippy" draggable="false" />
  </button>
</main>
