<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { emit, listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';

  let question = '';
  let response: { category: string; answer: string } | null = null;
  let searching = false;
  let chatWindow: HTMLElement;

  async function resizeChat() {
    await tick();
    const contentHeight = Math.ceil(chatWindow.scrollHeight + 18);
    await invoke('resize_chat', { height: contentHeight });
  }

  onMount(() => {
    void resizeChat();

    let unlisten: (() => void) | undefined;
    void listen<{ context: string; response: string }>('app-context-detected', (event) => {
      response = {
        category: event.payload.context,
        answer: event.payload.response
      };
      void resizeChat();
    }).then((stopListening) => {
      unlisten = stopListening;
    });

    return () => unlisten?.();
  });

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const query = question.trim();
    if (!query || searching) return;

    searching = true;
    try {
      const result = await invoke<{ category: string; answer: string }>('search_docs', { query });
      response = result;
      if (result.category === 'easteregg' && query.toLowerCase().includes('do a flip')) {
        await emit('clippy-flip');
      }
      await resizeChat();
    } finally {
      searching = false;
    }
  }

  async function closeChatWindow() {
    await invoke('close_chat');
  }

  async function snoozeInterruptions() {
    await invoke('snooze_interruptions');
    await closeChatWindow();
  }
</script>

<svelte:head>
  <title>Ask Clippy</title>
</svelte:head>

<svelte:window oncontextmenu={(event) => event.preventDefault()} />

<main class="chat-window" bind:this={chatWindow}>
  <header class="chat-header">
    <p class="chat-prompt">What would you like help with?</p>
  </header>
  <form class="chat-form" onsubmit={submit}>
    <input class="chat-input" bind:value={question} placeholder="Ask Clippy..." />
    <div class="chat-actions">
      <button class="chat-action-button chat-close-button" type="button" onclick={closeChatWindow}>Close</button>
      <button class="chat-action-button chat-snooze-button" type="button" onclick={snoozeInterruptions}>
        Don't interrupt · 15m
      </button>
      <button
        class="chat-action-button chat-submit"
        type="submit"
        disabled={searching || !question.trim()}
        aria-label="Ask Clippy"
        title="Ask Clippy"
      >
        {searching ? '…' : '→'}
      </button>
    </div>
  </form>

  {#if response}
    <section class="chat-response" aria-live="polite">
      <p>{response.answer}</p>
    </section>
  {/if}
</main>
