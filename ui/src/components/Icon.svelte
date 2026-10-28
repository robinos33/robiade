<script lang="ts">
  let { name, size = 16 }: { name: string; size?: number } = $props();
</script>

<svg viewBox="0 0 16 16" width={size} height={size} aria-hidden="true">
  {#if name === 'inbox'}
    <path d="M2 9.5 3.6 3.2c.1-.4.5-.7 1-.7h6.8c.5 0 .9.3 1 .7L14 9.5V13a.5.5 0 0 1-.5.5h-11A.5.5 0 0 1 2 13z M2 9.5h3.5l1 1.5h3l1-1.5H14" />
  {:else if name === 'agents'}
    <rect x="1.5" y="2.5" width="13" height="11" rx="2" /><path d="m4.5 6.5 2 1.5-2 1.5M8.5 10h3" />
  {:else if name === 'history'}
    <path d="M2.5 8a5.5 5.5 0 1 0 1.6-3.9M2.5 2.5v2.2h2.2M8 5v3.2l2 1.3" />
  {:else if name === 'tasks'}
    <path d="m2.5 4 1.2 1.2L6 3M2.5 9l1.2 1.2L6 8M8.5 4.5h5M8.5 9.5h5M8.5 13h5" />
  {:else if name === 'stats'}
    <path d="M2.5 13.5h11M4.5 11V8M8 11V4M11.5 11V6.5" />
  {:else if name === 'mr'}
    <circle cx="4" cy="3.5" r="1.6" /><circle cx="4" cy="12.5" r="1.6" /><circle cx="12" cy="12.5" r="1.6" /><path d="M4 5.1v5.8M12 10.9V6.5c0-1.4-1-2.5-2.5-2.5H7" />
  {:else if name === 'env'}
    <rect x="2" y="2.5" width="12" height="4.5" rx="1.2" /><rect x="2" y="9" width="12" height="4.5" rx="1.2" /><path d="M4.5 4.75h.01M4.5 11.25h.01" />
  {:else if name === 'db'}
    <ellipse cx="8" cy="3.8" rx="5" ry="1.8" /><path d="M3 3.8v8.4c0 1 2.2 1.8 5 1.8s5-.8 5-1.8V3.8M3 8c0 1 2.2 1.8 5 1.8s5-.8 5-1.8" />
  {:else if name === 'plus'}
    <path d="M8 3v10M3 8h10" />
  {:else if name === 'search'}
    <circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" />
  {:else if name === 'folder'}
    <path d="M1.5 4.5c0-.6.4-1 1-1h3.3l1.5 1.5h6.2c.6 0 1 .4 1 1v6.5c0 .6-.4 1-1 1h-11c-.6 0-1-.4-1-1z" />
  {:else if name === 'globe'}
    <circle cx="8" cy="8" r="6" /><path d="M2 8h12M8 2c1.8 1.7 2.6 3.7 2.6 6S9.8 12.3 8 14c-1.8-1.7-2.6-3.7-2.6-6S6.2 3.7 8 2" />
  {:else if name === 'stop'}
    <rect x="4" y="4" width="8" height="8" rx="1.5" />
  {:else if name === 'resume'}
    <path d="M5 3.5v9l7-4.5z" />
  {:else if name === 'branch'}
    <circle cx="4.5" cy="3.5" r="1.5" /><circle cx="4.5" cy="12.5" r="1.5" /><circle cx="11.5" cy="5.5" r="1.5" /><path d="M4.5 5v6M11.5 7c0 2.5-2 3-7 4" />
  {:else if name === 'trash'}
    <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5c0 .3.3.5.6.5h4.6c.3 0 .6-.2.6-.5l.6-8.5" />
  {:else if name === 'image'}
    <rect x="2" y="2.5" width="12" height="11" rx="1.5" /><circle cx="5.8" cy="6" r="1.2" /><path d="m2.5 12 3.8-3.6 2.4 2.2 2-1.8 2.8 2.7" />
  {:else if name === 'file'}
    <path d="M4 1.8h5l3 3V14a.5.5 0 0 1-.5.5h-7.5A.5.5 0 0 1 3.5 14V2.3c0-.3.2-.5.5-.5zM9 1.8v3h3M5.8 8.5h4.4M5.8 11h4.4" />
  {:else if name === 'pencil'}
    <path d="m10.5 2.8 2.7 2.7-7.7 7.7H2.8v-2.7zM9 4.3l2.7 2.7" />
  {:else if name === 'copy'}
    <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" /><path d="M10.5 5.5V3.5a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2" />
  {:else if name === 'close'}
    <path d="m4 4 8 8M12 4l-8 8" />
  {:else if name === 'lock'}
    <rect x="3" y="7" width="10" height="7" rx="1.5" /><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" />
  {/if}
</svg>

<style>
  svg { fill: none; stroke: currentColor; stroke-width: 1.4; stroke-linecap: round; stroke-linejoin: round; flex: none; }
</style>
