<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  let question = '';
  let response: { category: string; answer: string } | null = null;
  let searching = false;

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const query = question.trim();
    if (!query || searching) return;

    searching = true;
    try {
      response = await invoke('search_docs', { query });
    } finally {
      searching = false;
    }
  }

  async function closeChat() {
    await invoke('close_chat');
  }
</script>

<svelte:head>
  <title>Ask Clippy</title>
</svelte:head>

<svelte:window oncontextmenu={(event) => event.preventDefault()} />

<main class="chat-window">
  <header class="chat-header">
    <p class="chat-prompt">What would you like help with?</p>
    <button class="chat-close" onclick={closeChat} aria-label="Close chat">×</button>
  </header>
  <form class="chat-form" onsubmit={submit}>
    <input class="chat-input" bind:value={question} placeholder="Ask Clippy..." />
    <button class="chat-submit" type="submit" disabled={searching || !question.trim()}>
      {searching ? 'Thinking...' : 'Ask'}
    </button>
  </form>

  {#if response}
    <section class="chat-response" aria-live="polite">
      <span class="chat-category">{response.category}</span>
      <p>{response.answer}</p>
    </section>
  {/if}
</main>
