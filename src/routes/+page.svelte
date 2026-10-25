<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';

  const APP_CONTEXT_POLL_INTERVAL_MS = 5000;

  let previousContext = '';
  let flipping = false;

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

  async function checkForAppContext() {
    try {
      const context = await invoke<{ context: string } | null>('active_app_context');
      if (context && context.context !== previousContext) {
        await invoke('show_context_nudge', { context: context.context });
      }
      previousContext = context?.context ?? '';
    } catch (error) {
      console.error('[Re-Clippy] could not check active window', error);
    }
  }

  async function flipClippy() {
    flipping = false;
    await tick();
    flipping = true;
    window.setTimeout(() => (flipping = false), 700);
  }

  onMount(() => {
    void checkForAppContext();
    const interval = window.setInterval(() => void checkForAppContext(), APP_CONTEXT_POLL_INTERVAL_MS);
    let unlisten: (() => void) | undefined;
    void listen('clippy-flip', () => void flipClippy()).then((stopListening) => {
      unlisten = stopListening;
    });

    return () => {
      window.clearInterval(interval);
      unlisten?.();
    };
  });
</script>

<svelte:head>
  <title>Re-Clippy</title>
</svelte:head>

<svelte:window oncontextmenu={(event) => event.preventDefault()} />

<main class="clippy-window">
  <button class="clippy-button" onpointerdown={logPointerDown} onclick={openChat} aria-label="Open Clippy chat">
    <img class:clippy-flip={flipping} class="clippy-image" src="/clippy.webp" alt="Clippy" draggable="false" />
  </button>
</main>
