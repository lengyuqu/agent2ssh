import { FileKey, Plus } from "lucide-react";
import { FormEvent, useState } from "react";
import { api } from "../api";

const emptyForm = { name: "", host: "", user: "", port: 22, key_path: "" };

type Props = {
  onSaved: () => void;
};

export default function AddHostForm({ onSaved }: Props) {
  const [form, setForm] = useState(emptyForm);

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    await api.addHost({
      name: form.name.trim(),
      host: form.host.trim(),
      user: form.user.trim() || null,
      port: form.port || null,
      key_path: form.key_path.trim() || null,
    });
    setForm(emptyForm);
    onSaved();
  }

  return (
    <section className="panel">
      <div className="panel-title">
        <Plus size={16} />
        Add Host
      </div>
      <form className="host-form" onSubmit={handleSubmit}>
        <label>
          Alias
          <input
            required
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder="prod"
          />
        </label>
        <label>
          Host
          <input
            required
            value={form.host}
            onChange={(e) => setForm({ ...form, host: e.target.value })}
            placeholder="10.0.0.12"
          />
        </label>
        <div className="two-col">
          <label>
            User
            <input
              value={form.user}
              onChange={(e) => setForm({ ...form, user: e.target.value })}
              placeholder="ubuntu"
            />
          </label>
          <label>
            Port
            <input
              type="number"
              min={1}
              max={65535}
              value={form.port}
              onChange={(e) =>
                setForm({ ...form, port: Number(e.target.value) })
              }
            />
          </label>
        </div>
        <label>
          Key path
          <input
            value={form.key_path}
            onChange={(e) => setForm({ ...form, key_path: e.target.value })}
            placeholder="~/.ssh/id_ed25519"
          />
        </label>
        <button className="secondary" type="submit">
          <FileKey size={16} />
          Save host
        </button>
      </form>
    </section>
  );
}
