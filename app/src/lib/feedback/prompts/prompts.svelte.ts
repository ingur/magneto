// Prompts: awaitable confirm/text/choice/info dialogs.
//
//   const ok = await prompt({ type: 'confirm', title: '...' });
//   const name = await prompt({ type: 'text', title: '...', default: '' });
//   const pick = await prompt({ type: 'choice', title: '...', choices: [...] });
//   await prompt({ type: 'info', title: '...', description: '...' });
//
// Returns:
//   confirm → true (confirmed) | null (cancelled)
//   text    → string (committed) | null (cancelled)
//   choice  → string (selected) | null (cancelled)
//   info    → null (always; info dialogs are dismiss-only, no buttons)
//
// One prompt is shown at a time; further calls queue and resolve in order.
// Each visible prompt pushes its own KbLayer (handled in PromptStack.svelte),
// so all the cursor/activate/escape semantics are inherited from the layer
// system, nothing prompt-specific in kb.

export type ConfirmOpts = {
  type: "confirm";
  title: string;
  description?: string;
  confirmLabel?: string; // default 'OK'
  cancelLabel?: string; // default 'Cancel'
  tint?: "accent" | "info" | "danger"; // confirm-button tint; default 'accent'
};

export type TextOpts = {
  type: "text";
  title: string;
  description?: string;
  default?: string;
  placeholder?: string;
  confirmLabel?: string; // default 'OK'
  cancelLabel?: string; // default 'Cancel'
};

export type ChoiceOpts = {
  type: "choice";
  title: string;
  description?: string;
  choices: string[];
  default?: string;
  confirmLabel?: string; // default 'OK'
  cancelLabel?: string; // default 'Cancel'
};

// Pure-information dialog: title + body, optional single action button.
// Dismiss with Esc or backdrop click (always; esc never invokes the
// action, it's the safe out). With an action, Enter / Space invoke it
// (cursor lands on the button). Without, Enter / Space also dismiss.
//
// Use for About / version / static notices. The optional action covers
// "info + one outbound link" cases (donate, repo, etc.) without needing
// the full confirm/cancel pair: there's nothing to cancel, the user
// either takes the action or just reads.
import type { Component } from "svelte";

export type InfoAction = {
  label: string;
  tint?: "accent" | "info" | "danger";
  // Optional leading icon. Component, not snippet: keeps InfoOpts
  // serializable-ish and matches Menu/MenuItem's icon convention.
  icon?: Component;
  onClick: () => void;
};

export type InfoOpts = {
  type: "info";
  title: string;
  description?: string;
  action?: InfoAction;
};

export type PromptOpts = ConfirmOpts | TextOpts | ChoiceOpts | InfoOpts;
export type PromptResult = boolean | string | null;

export type PendingPrompt = {
  id: number;
  opts: PromptOpts;
  resolve: (v: PromptResult) => void;
};

let nextId = 0;

class PromptsStore {
  queue = $state<PendingPrompt[]>([]);

  get current(): PendingPrompt | null {
    return this.queue[0] ?? null;
  }

  show(opts: PromptOpts): Promise<PromptResult> {
    return new Promise((resolve) => {
      this.queue.push({ id: ++nextId, opts, resolve });
    });
  }

  // Resolve the head of the queue and remove it. Single source of truth
  // for "this prompt is done"; the renderer calls this on confirm/cancel.
  resolve(value: PromptResult) {
    const p = this.queue.shift();
    p?.resolve(value);
  }
}

export const prompts = new PromptsStore();

export function prompt(opts: PromptOpts): Promise<PromptResult> {
  return prompts.show(opts);
}
