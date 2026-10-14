import { useSignal } from "@preact/signals";
import { useEffect, useRef } from "preact/hooks";
import { api } from "../lib/api";
import { errorText } from "../lib/errors";
import { t } from "../lib/i18n";
import { Check, Sparkles } from "./icons";
import styles from "./Styles.module.css";

export interface ImproveStyleProps {
  id: string;
  name: string;
  keyword: string | null;
  /** The task shown in the card. */
  draft: string;
  onAccept: (task: string) => void;
  onClose: () => void;
}

/** Asks the corrector for a new task from a request and the current draft; the user previews it before using it. */
export default function ImproveStyle({ id, name, keyword, draft, onAccept, onClose }: ImproveStyleProps) {
  const instructions = useSignal("");
  const pending = useSignal(false);
  const proposal = useSignal<string | null>(null);
  const error = useSignal<unknown>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  useEffect(() => field.current!.focus(), []);

  async function run() {
    pending.value = true;
    error.value = null;
    proposal.value = null;
    try {
      proposal.value = await api.improveStyle({ name, keyword, draft, instructions: instructions.value });
    } catch (e) {
      error.value = e;
    } finally {
      pending.value = false;
    }
  }

  function accept() {
    onAccept(proposal.value!);
    onClose();
  }

  const m = t.value.styles.improve;
  return (
    <div id={id} class={styles.improve}>
      <label class={styles.task}>
        <span>{m.request}</span>
        <textarea
          ref={field}
          placeholder={m.placeholder}
          value={instructions.value}
          onInput={(e) => (instructions.value = e.currentTarget.value)}
        />
      </label>
      <div class={styles.buttons}>
        <button type="button" class="ghost" onClick={onClose} disabled={pending.value}>
          {m.cancel}
        </button>
        <button type="button" class="primary" onClick={run} disabled={pending.value || instructions.value.trim() === ""}>
          <Sparkles />
          {pending.value ? m.running : m.run}
        </button>
      </div>
      {error.value !== null && (
        <p class={styles.error} role="alert">
          {m.failed(errorText(error.value))}
        </p>
      )}
      {proposal.value !== null && (
        <div role="group" aria-label={m.proposal} class={styles.proposalGroup}>
          <span class={styles.label}>{m.proposal}</span>
          <pre class={styles.proposal}>{proposal.value}</pre>
          <div class={styles.buttons}>
            <button type="button" class="ghost" onClick={() => (proposal.value = null)}>
              {m.discard}
            </button>
            <button type="button" class="primary" onClick={accept}>
              <Check />
              {m.use}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
