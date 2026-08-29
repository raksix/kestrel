import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  analyzeLastCapture,
  compareHash,
  generateQrCode,
  hashFile,
  scanQrCode,
  type Analysis,
  type DecodedQr,
  type FileHash,
} from "../../lib/ipc";
import "./tools.css";

export default function Tools() {
  return (
    <div className="stack">
      <QrTool />
      <HashTool />
      <AnalyzeTool />
    </div>
  );
}

function QrTool() {
  const [text, setText] = useState("https://github.com/raksix/kestrel");
  const [image, setImage] = useState<string | null>(null);
  const [found, setFound] = useState<DecodedQr[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (!text.trim()) {
      setImage(null);
      return;
    }
    generateQrCode(text)
      .then((data) => {
        if (!cancelled) {
          setImage(data);
          setError(null);
        }
      })
      .catch((e) => {
        if (!cancelled) {
          setImage(null);
          setError(String(e));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [text]);

  return (
    <section className="card">
      <h2 className="card__title">QR kod</h2>
      <p className="card__hint">
        Metni QR koda çevir, ya da son yakalamadaki kodları oku.
      </p>

      <div className="tools__row">
        <input
          className="input"
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder="Kodlanacak metin veya URL"
        />
        <button
          type="button"
          className="button"
          onClick={async () => {
            try {
              setFound(await scanQrCode());
              setError(null);
            } catch (e) {
              setError(String(e));
            }
          }}
        >
          Son yakalamayı tara
        </button>
      </div>

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}

      {image && <img className="tools__qr" src={image} alt="Üretilen QR kod" />}

      {found !== null &&
        (found.length === 0 ? (
          <p className="muted">Son yakalamada QR kod bulunamadı.</p>
        ) : (
          <ul className="tools__results">
            {found.map((entry, index) => (
              <li key={index} className="mono">
                {entry.text}
              </li>
            ))}
          </ul>
        ))}
    </section>
  );
}

function HashTool() {
  const [path, setPath] = useState<string | null>(null);
  const [hashes, setHashes] = useState<FileHash[]>([]);
  const [expected, setExpected] = useState("");
  const [verdict, setVerdict] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Comparison happens in Rust so the UI and the checker agree on what counts
  // as a match — case and stray whitespace included.
  useEffect(() => {
    if (!expected.trim() || hashes.length === 0) {
      setVerdict(null);
      return;
    }
    let cancelled = false;
    Promise.all(hashes.map((h) => compareHash(expected, h.digest)))
      .then((results) => {
        if (!cancelled) setVerdict(results.some(Boolean));
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [expected, hashes]);

  return (
    <section className="card">
      <h2 className="card__title">Hash kontrolü</h2>
      <p className="card__hint">MD5, SHA-1, SHA-256 ve SHA-512 tek geçişte hesaplanır.</p>

      <div className="tools__row">
        <button
          type="button"
          className="button"
          onClick={async () => {
            const chosen = await open({ multiple: false });
            if (typeof chosen !== "string") return;
            setPath(chosen);
            try {
              setHashes(await hashFile(chosen));
              setError(null);
            } catch (e) {
              setError(String(e));
              setHashes([]);
            }
          }}
        >
          Dosya seç…
        </button>
        {path && (
          <span className="muted tools__path" title={path}>
            {path.split(/[\\/]/).pop()}
          </span>
        )}
      </div>

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}

      {hashes.length > 0 && (
        <>
          <ul className="tools__results">
            {hashes.map((entry) => (
              <li key={entry.algorithm}>
                <span className="tools__algorithm">{entry.algorithm}</span>
                <span className="mono tools__digest">{entry.digest}</span>
              </li>
            ))}
          </ul>

          <label className="tools__row">
            <span className="visually-hidden">Beklenen hash</span>
            <input
              className="input input--mono"
              value={expected}
              placeholder="Beklenen hash'i yapıştır"
              onChange={(event) => setExpected(event.target.value)}
            />
          </label>

          {verdict !== null && (
            <p className={`status ${verdict ? "status--ok" : "status--error"}`} role="status">
              <span className="dot" aria-hidden="true" />
              {verdict ? "Eşleşiyor" : "Eşleşmiyor"}
            </p>
          )}
        </>
      )}
    </section>
  );
}

function AnalyzeTool() {
  const [analysis, setAnalysis] = useState<Analysis | null>(null);
  const [error, setError] = useState<string | null>(null);

  return (
    <section className="card">
      <h2 className="card__title">Görsel analizi</h2>
      <p className="card__hint">Son yakalamanın boyutu, renkleri ve şeffaflığı.</p>

      <button
        type="button"
        className="button"
        onClick={async () => {
          try {
            setAnalysis(await analyzeLastCapture());
            setError(null);
          } catch (e) {
            setError(String(e));
          }
        }}
      >
        İncele
      </button>

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}

      {analysis && (
        <ul className="tools__results">
          <li>
            <span className="tools__algorithm">Boyut</span>
            <span className="mono">
              {analysis.width} × {analysis.height}
            </span>
          </li>
          <li>
            <span className="tools__algorithm">Renk sayısı</span>
            <span className="mono">
              {analysis.uniqueColours.toLocaleString()}
              {analysis.uniqueColoursCapped ? "+" : ""}
            </span>
          </li>
          <li>
            <span className="tools__algorithm">Şeffaflık</span>
            <span className="mono">{analysis.hasTransparency ? "var" : "yok"}</span>
          </li>
          <li>
            <span className="tools__algorithm">Baskın renkler</span>
            <span className="tools__swatches">
              {analysis.dominant.map((colour) => (
                <span
                  key={colour}
                  className="tools__swatch"
                  style={{ background: colour }}
                  title={colour}
                />
              ))}
            </span>
          </li>
        </ul>
      )}
    </section>
  );
}
