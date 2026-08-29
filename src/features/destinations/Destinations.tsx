import { useCallback, useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import {
  defaultDestination,
  importUploader,
  listDestinations,
  removeUploader,
  setDefaultDestination,
  type Destination,
} from "../../lib/ipc";
import "./destinations.css";

const SXCU_CATALOG = "https://github.com/ShareX/CustomUploaders";

/** What a destination accepts, as a short human list. */
function accepts(destination: Destination): string {
  const kinds = [
    destination.acceptsImage && "görsel",
    destination.acceptsFile && "dosya",
    destination.acceptsText && "metin",
    destination.shortensUrls && "URL kısaltma",
  ].filter(Boolean);
  return kinds.length > 0 ? kinds.join(", ") : "belirtilmemiş";
}

export default function Destinations() {
  const [destinations, setDestinations] = useState<Destination[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [dropping, setDropping] = useState(false);

  const refresh = useCallback(() => {
    listDestinations()
      .then(setDestinations)
      .catch((e) => setError(String(e)));
    // The choice is persisted, so it has to be read back rather than assumed
    // to be unset every time this panel mounts.
    defaultDestination()
      .then(setSelected)
      .catch(() => undefined);
  }, []);

  useEffect(refresh, [refresh]);

  const importFiles = useCallback(
    async (paths: string[]) => {
      const files = paths.filter((path) => path.toLowerCase().endsWith(".sxcu"));
      if (files.length === 0) {
        setError("Sadece .sxcu dosyaları içe aktarılabilir.");
        return;
      }

      const failures: string[] = [];
      for (const path of files) {
        try {
          await importUploader(path);
        } catch (e) {
          // One bad file must not abandon the rest of a multi-file drop.
          failures.push(`${path.split(/[\\/]/).pop()}: ${e}`);
        }
      }

      refresh();
      setError(failures.length > 0 ? failures.join("\n") : null);
      const added = files.length - failures.length;
      if (added > 0) {
        setNotice(`${added} uploader eklendi`);
        window.setTimeout(() => setNotice(null), 2500);
      }
    },
    [refresh],
  );

  // Dropping a .sxcu onto the window is how people share these, so it has to
  // work anywhere in the panel, not just on a small target.
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "over") setDropping(true);
      else if (event.payload.type === "leave") setDropping(false);
      else if (event.payload.type === "drop") {
        setDropping(false);
        void importFiles(event.payload.paths);
      }
    });
    return () => {
      unlisten.then((un) => un()).catch(() => undefined);
    };
  }, [importFiles]);

  return (
    <div className="stack">
      <section className={`destinations__drop ${dropping ? "destinations__drop--over" : ""}`}>
        <h2 className="card__title">Custom uploader ekle</h2>
        <p className="card__hint">
          ShareX'in <code>.sxcu</code> dosyaları olduğu gibi çalışır — sürükleyip bırak ya
          da dosya seç. Hazır uploader'lar için{" "}
          <a href={SXCU_CATALOG} target="_blank" rel="noreferrer">
            ShareX/CustomUploaders
          </a>
          .
        </p>
        <button
          type="button"
          className="button"
          onClick={async () => {
            const chosen = await open({
              multiple: true,
              filters: [{ name: "ShareX custom uploader", extensions: ["sxcu"] }],
            });
            if (!chosen) return;
            await importFiles(Array.isArray(chosen) ? chosen : [chosen]);
          }}
        >
          Dosya seç…
        </button>
      </section>

      {error && (
        <p className="status status--error" role="alert" style={{ whiteSpace: "pre-line" }}>
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}
      {notice && (
        <p className="status status--ok" role="status">
          <span className="dot" aria-hidden="true" />
          {notice}
        </p>
      )}

      {destinations.length === 0 ? (
        <div className="card">
          <h2 className="card__title">Henüz hedef yok</h2>
          <p className="card__hint">
            Bir <code>.sxcu</code> dosyası ekleyene kadar yükleme yapılamaz.
          </p>
        </div>
      ) : (
        <div className="grid">
          {destinations.map((destination) => (
            <article
              key={destination.id}
              className={`destinations__item ${
                selected === destination.id ? "destinations__item--default" : ""
              }`}
            >
              <div className="destinations__head">
                <span className="destinations__name">{destination.name}</span>
                {selected === destination.id && (
                  <span className="destinations__badge">Varsayılan</span>
                )}
              </div>
              <span className="destinations__host" title={destination.host}>
                {destination.host}
              </span>
              <span className="destinations__kinds">Kabul ettiği: {accepts(destination)}</span>

              <div className="destinations__actions">
                <button
                  type="button"
                  className="button"
                  onClick={async () => {
                    const next = selected === destination.id ? null : destination.id;
                    await setDefaultDestination(next);
                    setSelected(next);
                  }}
                >
                  {selected === destination.id ? "Varsayılanı kaldır" : "Varsayılan yap"}
                </button>
                <button
                  type="button"
                  className="button"
                  onClick={async () => {
                    try {
                      setDestinations(await removeUploader(destination.id));
                      if (selected === destination.id) {
                        await setDefaultDestination(null);
                        setSelected(null);
                      }
                    } catch (e) {
                      setError(String(e));
                    }
                  }}
                >
                  Sil
                </button>
              </div>
            </article>
          ))}
        </div>
      )}

      {destinations.length === 1 && selected === null && (
        <p className="muted">
          Tek hedef olduğu için ayrıca seçmene gerek yok; yüklemeler buraya gider.
        </p>
      )}
    </div>
  );
}
