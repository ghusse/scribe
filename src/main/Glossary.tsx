import { useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api, type Term } from "../lib/api";
import { joinVariants, noteOrNull, splitVariants } from "../lib/glossary";
import styles from "./Glossary.module.css";

export type GlossaryProps = Record<string, never>;

type Draft = { term: string; variants: string; note: string };

const inputValue = (e: Event) => (e.currentTarget as HTMLInputElement).value;

export default function Glossary(_props: GlossaryProps) {
  const terms = useSignal<Term[]>([]);
  const term = useSignal("");
  const variants = useSignal("");
  const note = useSignal("");
  const error = useSignal<string | null>(null);
  const editId = useSignal<number | null>(null);
  const edit = useSignal<Draft>({ term: "", variants: "", note: "" });

  async function load() {
    try {
      terms.value = await api.listTerms();
    } catch (err) {
      error.value = String(err);
    }
  }

  async function add(e: Event) {
    e.preventDefault();
    try {
      await api.addTerm(term.value, splitVariants(variants.value), noteOrNull(note.value));
      term.value = variants.value = note.value = "";
      error.value = null;
      await load();
    } catch (err) {
      error.value = String(err);
    }
  }

  function startEdit(t: Term) {
    editId.value = t.id;
    edit.value = { term: t.term, variants: joinVariants(t.variants), note: t.note ?? "" };
  }

  const setEdit = (field: keyof Draft) => (e: Event) => {
    edit.value = { ...edit.value, [field]: inputValue(e) };
  };

  async function saveEdit(id: number) {
    try {
      const d = edit.value;
      await api.updateTerm(id, d.term, splitVariants(d.variants), noteOrNull(d.note));
      editId.value = null;
      error.value = null;
      await load();
    } catch (err) {
      error.value = String(err);
    }
  }

  async function remove(id: number) {
    try {
      await api.deleteTerm(id);
      error.value = null;
      await load();
    } catch (err) {
      error.value = String(err);
    }
  }

  useEffect(() => {
    void load();
  }, []);

  return (
    <>
      <form class={styles.form} onSubmit={add}>
        <input
          placeholder="Terme (ex. Kubernetes)"
          value={term.value}
          onInput={(e) => (term.value = inputValue(e))}
          required
        />
        <input
          placeholder="Mal entendu (séparés par des virgules)"
          value={variants.value}
          onInput={(e) => (variants.value = inputValue(e))}
        />
        <input
          placeholder="Note / contexte (optionnel)"
          value={note.value}
          onInput={(e) => (note.value = inputValue(e))}
        />
        <button class="primary" type="submit">
          Ajouter
        </button>
      </form>
      {error.value && <p class={styles.error}>{error.value}</p>}

      <table class={styles.table}>
        <thead>
          <tr>
            <th>Terme</th>
            <th>Entendu</th>
            <th>Note</th>
            <th>Usages</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {terms.value.length === 0 ? (
            <tr>
              <td colSpan={5} class={styles.muted}>
                Glossaire vide : ajoutez les noms propres et le jargon de vos domaines.
              </td>
            </tr>
          ) : (
            terms.value.map((t) => (
              <tr key={t.id}>
                {editId.value === t.id ? (
                  <>
                    <td>
                      <input value={edit.value.term} onInput={setEdit("term")} />
                    </td>
                    <td>
                      <input value={edit.value.variants} onInput={setEdit("variants")} />
                    </td>
                    <td>
                      <input value={edit.value.note} onInput={setEdit("note")} />
                    </td>
                    <td>{t.use_count}</td>
                    <td>
                      <button class="primary" onClick={() => saveEdit(t.id)}>
                        OK
                      </button>{" "}
                      <button onClick={() => (editId.value = null)}>Annuler</button>
                    </td>
                  </>
                ) : (
                  <>
                    <td>
                      <strong>{t.term}</strong>
                    </td>
                    <td>{joinVariants(t.variants)}</td>
                    <td>{t.note ?? ""}</td>
                    <td>{t.use_count}</td>
                    <td>
                      <button onClick={() => startEdit(t)}>Modifier</button>{" "}
                      <button class="danger" onClick={() => remove(t.id)}>
                        Supprimer
                      </button>
                    </td>
                  </>
                )}
              </tr>
            ))
          )}
        </tbody>
      </table>
    </>
  );
}
