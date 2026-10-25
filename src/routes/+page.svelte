<svelte:head>
  <title>Re-Clippy</title>
</svelte:head>

<svelte:window oncontextmenu={(event) => event.preventDefault()} />

<main>
  <img src="/clippy.webp" alt="Clippy" draggable="false" />
</main>

<style>
  :global(html),
  :global(body) {
    width: 100%;
    height: 100%;
    margin: 0;
    overflow: hidden;
    background: transparent !important;
  }

  main {
    width: 100vw;
    height: 100vh;
    display: flex;
    align-items: flex-end;
    justify-content: flex-end;
  }

  img {
    display: block;
    width: auto;
    height: 100%;
    max-width: 100%;
    object-fit: contain;
    object-position: bottom right;
    user-select: none;
    -webkit-user-drag: none;
  }
</style>
