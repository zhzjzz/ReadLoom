<script module lang="ts">
  export type IconName =
    | 'book'
    | 'check'
    | 'chevron'
    | 'document'
    | 'info'
    | 'input'
    | 'library'
    | 'monitor'
    | 'moon'
    | 'settings'
    | 'sun';
</script>

<script lang="ts">
  export let name: IconName;
  export let size = 18;
</script>

<svg
  aria-hidden="true"
  class="icon"
  fill="none"
  height={size}
  viewBox="0 0 24 24"
  width={size}
>
  {#if name === 'book'}
    <path d="M4 5.5A3.5 3.5 0 0 1 7.5 2H11v17H7.5A3.5 3.5 0 0 0 4 22V5.5Z" />
    <path d="M20 5.5A3.5 3.5 0 0 0 16.5 2H13v17h3.5A3.5 3.5 0 0 1 20 22V5.5Z" />
  {:else if name === 'check'}
    <path d="m5 12 4 4L19 6" />
  {:else if name === 'chevron'}
    <path d="m9 18 6-6-6-6" />
  {:else if name === 'document'}
    <path d="M6 2h8l4 4v16H6V2Z" />
    <path d="M14 2v5h4M9 12h6M9 16h6" />
  {:else if name === 'info'}
    <circle cx="12" cy="12" r="9" />
    <path d="M12 11v6M12 7h.01" />
  {:else if name === 'input'}
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M7 9h10M7 13h6M7 17h3" />
  {:else if name === 'library'}
    <path d="M4 4h4v16H4zM10 4h4v16h-4zM16 6l3.5-1 3 13-3.5 1z" />
  {:else if name === 'monitor'}
    <rect x="3" y="4" width="18" height="13" rx="2" />
    <path d="M8 21h8M12 17v4" />
  {:else if name === 'moon'}
    <path d="M20 15.5A8.5 8.5 0 0 1 8.5 4 8.5 8.5 0 1 0 20 15.5Z" />
  {:else if name === 'settings'}
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6 1.7 1.7 0 0 0-.4 1.1V21h-4v-.09A1.7 1.7 0 0 0 8 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-.6-1 1.7 1.7 0 0 0-1.1-.4H3v-4h.09A1.7 1.7 0 0 0 4.6 8a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-.6 1.7 1.7 0 0 0 .4-1.1V3h4v.09A1.7 1.7 0 0 0 16 4.6a1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 9c.16.37.37.7.6 1 .28.31.67.5 1.1.5H21v4h-.09A1.7 1.7 0 0 0 19.4 15Z" />
  {:else if name === 'sun'}
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2v2M12 20v2M4.93 4.93l1.42 1.42M17.65 17.65l1.42 1.42M2 12h2M20 12h2M4.93 19.07l1.42-1.42M17.65 6.35l1.42-1.42" />
  {/if}
</svg>

<style>
  .icon {
    display: block;
    flex: 0 0 auto;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 1.7;
  }
</style>
