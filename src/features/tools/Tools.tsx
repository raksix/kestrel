import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  analyzeLastCapture,
  compareHash,
  combineImages,
  compareImages,
  convertVideo,
  defaultConvertSettings,
  generateQrCode,
  hashFile,
  ocrInstall,
  ocrLastCapture,
  ocrStatus,
  parseColor,
  pickColor,
  scanQrCode,
  splitImage,
  videoThumbnail,
  type Analysis,
  type DecodedQr,
  type FileHash,
  type ConvertSettings,
  type ConvertTarget,
  type ImageComparison,
  type OcrModelStatus,
  type Recognised,
  type Swatch,
} from "../../lib/ipc";
import "./tools.css";

export default function Tools() {
  return (
    <div className="stack">
      <QrTool />
      <ColorTool />
      <CompareTool />
      <CombineTool />
      <VideoTool />
      <OcrTool />
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


/**
 * ShareX's OCR, minus the Windows OCR API.
 *
 * The models are a 20 MB download, so this says so and waits to be told rather
 * than reaching out to the network the first time someone clicks the button.
 */
function OcrTool() {
  const [status, setStatus] = useState<OcrModelStatus | null>(null);
  const [result, setResult] = useState<Recognised | null>(null);
  const [busy, setBusy] = useState<"install" | "read" | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    ocrStatus().then(setStatus).catch((e) => setError(String(e)));
  }, []);

  const install = async () => {
    setBusy("install");
    setError(null);
    try {
      setStatus(await ocrInstall());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const read = async () => {
    setBusy("read");
    setError(null);
    try {
      setResult(await ocrLastCapture());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className="card">
      <h2 className="card__title">Metin tanıma (OCR)</h2>
      <p className="card__hint">
        Son yakalamadaki metni oku. Tanıma tamamen bu makinede çalışır, görsel
        hiçbir yere gönderilmez.
      </p>

      {status && !status.installed && (
        <>
          <p className="card__hint">
            Modeller kurulu değil. Kurulum yaklaşık {status.downloadSizeMb} MB
            indirir; bu tek seferlik ve indirmeyi sen başlatana kadar ağa
            çıkılmaz.
          </p>
          <button
            type="button"
            className="button button--primary"
            disabled={busy !== null}
            onClick={install}
          >
            {busy === "install" ? "İndiriliyor…" : "Modelleri indir"}
          </button>
        </>
      )}

      {status?.installed && (
        <button
          type="button"
          className="button button--primary"
          disabled={busy !== null}
          onClick={read}
        >
          {busy === "read" ? "Okunuyor…" : "Son yakalamayı oku"}
        </button>
      )}

      {result && result.lines.length === 0 && (
        <p className="card__hint">Görselde okunabilir metin bulunamadı.</p>
      )}

      {result && result.lines.length > 0 && (
        <>
          <textarea className="input" readOnly rows={8} value={result.text} />
          <p className="card__hint">
            {result.lines.length} satır tanındı. Metin geçmişe de yazıldı, artık
            kütüphanede aratabilirsin.
          </p>
        </>
      )}

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}
    </section>
  );
}


/**
 * ShareX's colour picker.
 *
 * One colour in every notation at once, because the reason you picked it
 * decides which one you need — CSS wants hex, a design tool wants HSL.
 */
function ColorTool() {
  const [swatch, setSwatch] = useState<Swatch | null>(null);
  const [hex, setHex] = useState("#4C8DFF");
  const [point, setPoint] = useState({ x: 0, y: 0 });
  const [radius, setRadius] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    parseColor(hex).then(setSwatch).catch(() => {
      // A half-typed hex is not an error worth shouting about; the panel just
      // keeps showing the last colour that parsed.
    });
  }, [hex]);

  const fromCapture = async () => {
    try {
      const picked = await pickColor(point.x, point.y, radius || undefined);
      setSwatch(picked);
      setHex(picked.hex);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const rows: [string, string][] = swatch
    ? [
        ["HEX", swatch.hex],
        ["RGB", `${swatch.rgb.r}, ${swatch.rgb.g}, ${swatch.rgb.b}`],
        [
          "HSL",
          swatch.hsl.map((v, i) => (i === 0 ? Math.round(v) + "°" : Math.round(v) + "%")).join(", "),
        ],
        [
          "HSV",
          swatch.hsv.map((v, i) => (i === 0 ? Math.round(v) + "°" : Math.round(v) + "%")).join(", "),
        ],
        ["CMYK", swatch.cmyk.map((v) => Math.round(v) + "%").join(", ")],
      ]
    : [];

  return (
    <section className="card">
      <h2 className="card__title">Renk seçici</h2>
      <p className="card__hint">
        Bir renk yaz, ya da son yakalamadaki bir noktadan al. Her gösterim
        birden hesaplanır.
      </p>

      <label className="tools__row">
        <span className="tools__label">Renk</span>
        <input
          type="color"
          value={swatch?.hex ?? hex}
          onChange={(e) => setHex(e.target.value)}
        />
        <input
          className="input"
          value={hex}
          spellCheck={false}
          onChange={(e) => setHex(e.target.value)}
        />
      </label>

      {swatch && (
        <div
          className="color__preview"
          style={{
            background: swatch.hex,
            color: `rgb(${swatch.contrasting.r}, ${swatch.contrasting.g}, ${swatch.contrasting.b})`,
          }}
        >
          {swatch.hex}
        </div>
      )}

      <table className="color__table">
        <tbody>
          {rows.map(([label, value]) => (
            <tr key={label}>
              <th scope="row">{label}</th>
              <td>
                <code>{value}</code>
              </td>
              <td>
                <button
                  type="button"
                  className="button"
                  onClick={() => void navigator.clipboard.writeText(value)}
                >
                  Kopyala
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <div className="tools__row">
        <label className="tools__row">
          <span className="tools__label">X</span>
          <input
            type="number"
            className="input"
            min={0}
            value={point.x}
            onChange={(e) => setPoint({ ...point, x: Math.max(0, Number(e.target.value)) })}
          />
        </label>
        <label className="tools__row">
          <span className="tools__label">Y</span>
          <input
            type="number"
            className="input"
            min={0}
            value={point.y}
            onChange={(e) => setPoint({ ...point, y: Math.max(0, Number(e.target.value)) })}
          />
        </label>
        <label className="tools__row">
          <span className="tools__label">Ortalama yarıçapı</span>
          <input
            type="number"
            className="input"
            min={0}
            max={32}
            value={radius}
            onChange={(e) => setRadius(Math.max(0, Number(e.target.value)))}
          />
        </label>
      </div>
      <p className="card__hint">
        Yarıçap 0 tek pikseli okur. Kenar yumuşatılmış yazıda tek piksel
        genelde kimsenin kastettiği renk değildir; 1–2 vermek onu düzeltir.
      </p>
      <button type="button" className="button" onClick={fromCapture}>
        Son yakalamadan al
      </button>

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}
    </section>
  );
}

/** ShareX's image comparer: how much changed, and where. */
function CompareTool() {
  const [first, setFirst] = useState<string | null>(null);
  const [second, setSecond] = useState<string | null>(null);
  const [tolerance, setTolerance] = useState(4);
  const [result, setResult] = useState<ImageComparison | null>(null);
  const [error, setError] = useState<string | null>(null);

  const choose = async (set: (path: string) => void) => {
    const chosen = await open({
      filters: [{ name: "Görsel", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"] }],
    });
    if (chosen && !Array.isArray(chosen)) set(chosen);
  };

  const run = async () => {
    if (!first || !second) return;
    try {
      setResult(await compareImages(first, second, tolerance));
      setError(null);
    } catch (e) {
      setError(String(e));
      setResult(null);
    }
  };

  const name = (path: string | null) => path?.split(/[\\/]/).pop() ?? "seçilmedi";

  return (
    <section className="card">
      <h2 className="card__title">Görsel karşılaştır</h2>
      <p className="card__hint">
        İki görselin farkını yüzde olarak ve resim üzerinde göster. Farklı
        boyuttaki görseller ortak alanda karşılaştırılır.
      </p>

      <div className="tools__row">
        <button type="button" className="button" onClick={() => void choose(setFirst)}>
          1. görsel: {name(first)}
        </button>
        <button type="button" className="button" onClick={() => void choose(setSecond)}>
          2. görsel: {name(second)}
        </button>
      </div>

      <label className="tools__row">
        <span className="tools__label">Tolerans</span>
        <input
          type="range"
          min={0}
          max={32}
          value={tolerance}
          onChange={(e) => setTolerance(Number(e.target.value))}
        />
        <span className="muted">{tolerance}</span>
      </label>
      <p className="card__hint">
        Tolerans 0 yeniden kodlamadan gelen gürültüyü de fark sayar. Varsayılan
        4, JPEG artefaktlarını yutup gerçek değişikliği yakalar.
      </p>

      <button
        type="button"
        className="button button--primary"
        disabled={!first || !second}
        onClick={run}
      >
        Karşılaştır
      </button>

      {result && (
        <>
          <p className="card__hint">
            {result.changedPixels === 0
              ? "Karşılaştırılan alanda fark yok."
              : `%${result.differencePercent.toFixed(2)} farklı — ${result.changedPixels.toLocaleString("tr")} piksel, en büyük kanal farkı ${result.maxChannelDelta}.`}
          </p>
          {result.sizesDiffer && (
            <p className="card__hint">
              Boyutlar farklı; yalnızca {result.comparedWidth}×{result.comparedHeight}
              ortak alan karşılaştırıldı.
            </p>
          )}
          {result.bounds && (
            <p className="card__hint">
              Değişiklikler {result.bounds.x}, {result.bounds.y} noktasından
              başlayan {result.bounds.width}×{result.bounds.height} dikdörtgenin
              içinde.
            </p>
          )}
          <img className="compare__preview" src={result.preview} alt="Fark görseli" />
        </>
      )}

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}
    </section>
  );
}


const CONVERT_TARGETS: [ConvertTarget, string][] = [
  ["mp4", "MP4 (H.264)"],
  ["webm", "WebM (VP9)"],
  ["mkv", "MKV (H.264)"],
  ["gif", "GIF"],
  ["mp3", "MP3 (yalnız ses)"],
];

/** ShareX's video converter and thumbnailer, both ffmpeg. */
function VideoTool() {
  const [path, setPath] = useState<string | null>(null);
  const [settings, setSettings] = useState<ConvertSettings>(defaultConvertSettings());
  const [at, setAt] = useState(1);
  const [result, setResult] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const choose = async () => {
    const chosen = await open({
      filters: [{ name: "Video", extensions: ["mp4", "mkv", "webm", "mov", "avi", "gif"] }],
    });
    if (chosen && !Array.isArray(chosen)) {
      setPath(chosen);
      setResult(null);
    }
  };

  const run = async (task: () => Promise<string>) => {
    setBusy(true);
    setError(null);
    try {
      setResult(await task());
    } catch (e) {
      setError(String(e));
      setResult(null);
    } finally {
      setBusy(false);
    }
  };

  // A GIF has no audio and no CRF, and MP3 has no picture at all, so the
  // controls that do not apply are hidden rather than left there doing nothing.
  const isVideo = settings.target !== "mp3";
  const hasQuality = isVideo && settings.target !== "gif";

  return (
    <section className="card">
      <h2 className="card__title">Video dönüştür</h2>
      <p className="card__hint">
        Biçim değiştir, küçült, sesi ayır ya da tek kare al. Sonuç kaynağın
        yanına yazılır; kaynak dosyaya dokunulmaz.
      </p>

      <button type="button" className="button" onClick={choose}>
        {path ? path.split(/[\\/]/).pop() : "Video seç…"}
      </button>

      <div className="tools__row">
        <label className="tools__row">
          <span className="tools__label">Biçim</span>
          <select
            className="input"
            value={settings.target}
            onChange={(e) =>
              setSettings({ ...settings, target: e.target.value as ConvertTarget })
            }
          >
            {CONVERT_TARGETS.map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>

        {hasQuality && (
          <label className="tools__row">
            <span className="tools__label">Kalite (CRF)</span>
            <input
              type="range"
              min={0}
              max={51}
              value={settings.crf}
              onChange={(e) => setSettings({ ...settings, crf: Number(e.target.value) })}
            />
            <span className="muted">{settings.crf}</span>
          </label>
        )}
      </div>

      {isVideo && (
        <div className="tools__row">
          <label className="tools__row">
            <span className="tools__label">Genişlik</span>
            <input
              type="number"
              className="input"
              min={0}
              placeholder="özgün"
              value={settings.width ?? ""}
              onChange={(e) =>
                setSettings({ ...settings, width: e.target.value ? Number(e.target.value) : null })
              }
            />
          </label>
          <label className="tools__row">
            <span className="tools__label">FPS</span>
            <input
              type="number"
              className="input"
              min={0}
              placeholder="özgün"
              value={settings.fps ?? ""}
              onChange={(e) =>
                setSettings({ ...settings, fps: e.target.value ? Number(e.target.value) : null })
              }
            />
          </label>
          {settings.target !== "gif" && (
            <label className="tools__row">
              <input
                type="checkbox"
                checked={settings.mute}
                onChange={(e) => setSettings({ ...settings, mute: e.target.checked })}
              />
              <span className="tools__label">Sesi at</span>
            </label>
          )}
        </div>
      )}
      <p className="card__hint">
        Genişlik ve FPS boş bırakılırsa kaynağınki korunur. Yükseklik oranı
        korumak için otomatik hesaplanır.
      </p>

      <div className="tools__row">
        <button
          type="button"
          className="button button--primary"
          disabled={!path || busy}
          onClick={() => path && run(() => convertVideo(path, settings))}
        >
          {busy ? "Çalışıyor…" : "Dönüştür"}
        </button>

        <label className="tools__row">
          <span className="tools__label">Kare saniyesi</span>
          <input
            type="number"
            className="input"
            min={0}
            step={0.5}
            value={at}
            onChange={(e) => setAt(Math.max(0, Number(e.target.value)))}
          />
        </label>
        <button
          type="button"
          className="button"
          disabled={!path || busy}
          onClick={() => path && run(() => videoThumbnail(path, at))}
        >
          Kare al
        </button>
      </div>

      {result && (
        <p className="card__hint">
          Yazıldı: <code className="tools__path">{result}</code>
        </p>
      )}

      {error && (
        <p className="status status--error" role="alert" style={{ whiteSpace: "pre-line" }}>
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}
    </section>
  );
}


/** ShareX's image combiner and splitter. */
function CombineTool() {
  const [paths, setPaths] = useState<string[]>([]);
  const [vertical, setVertical] = useState(true);
  const [spacing, setSpacing] = useState(0);
  const [grid, setGrid] = useState({ columns: 2, rows: 2 });
  const [result, setResult] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const choose = async () => {
    const chosen = await open({
      multiple: true,
      filters: [{ name: "Görsel", extensions: ["png", "jpg", "jpeg", "webp", "bmp"] }],
    });
    if (!chosen) return;
    setPaths(Array.isArray(chosen) ? chosen : [chosen]);
    setResult(null);
  };

  const run = async (task: () => Promise<string[]>) => {
    try {
      setResult(await task());
      setError(null);
    } catch (e) {
      setError(String(e));
      setResult(null);
    }
  };

  const name = (path: string) => path.split(/[\\/]/).pop();

  return (
    <section className="card">
      <h2 className="card__title">Görsel birleştir / böl</h2>
      <p className="card__hint">
        Birden çok görseli alt alta ya da yan yana birleştir, ya da tek görseli
        ızgaraya böl. Farklı boyuttakiler esnetilmez — başa hizalanır, boşluk
        saydam kalır; esnetilmiş bir ekran görüntüsü okunmaz olur.
      </p>

      <button type="button" className="button" onClick={choose}>
        {paths.length > 0 ? `${paths.length} görsel seçildi` : "Görsel seç…"}
      </button>
      {paths.length > 0 && (
        <p className="card__hint">{paths.map(name).join(", ")}</p>
      )}

      <div className="tools__row">
        <label className="tools__row">
          <input
            type="checkbox"
            checked={vertical}
            onChange={(e) => setVertical(e.target.checked)}
          />
          <span className="tools__label">Alt alta</span>
        </label>
        <label className="tools__row">
          <span className="tools__label">Boşluk</span>
          <input
            type="number"
            className="input"
            min={0}
            max={200}
            value={spacing}
            onChange={(e) => setSpacing(Math.max(0, Number(e.target.value)))}
          />
        </label>
        <button
          type="button"
          className="button button--primary"
          disabled={paths.length < 2}
          onClick={() =>
            run(async () => [await combineImages(paths, vertical, spacing)])
          }
        >
          Birleştir
        </button>
      </div>

      <div className="tools__row">
        <label className="tools__row">
          <span className="tools__label">Sütun</span>
          <input
            type="number"
            className="input"
            min={1}
            max={20}
            value={grid.columns}
            onChange={(e) =>
              setGrid({ ...grid, columns: Math.max(1, Number(e.target.value)) })
            }
          />
        </label>
        <label className="tools__row">
          <span className="tools__label">Satır</span>
          <input
            type="number"
            className="input"
            min={1}
            max={20}
            value={grid.rows}
            onChange={(e) => setGrid({ ...grid, rows: Math.max(1, Number(e.target.value)) })}
          />
        </label>
        <button
          type="button"
          className="button"
          disabled={paths.length !== 1}
          onClick={() => run(() => splitImage(paths[0], grid.columns, grid.rows))}
          title={paths.length === 1 ? undefined : "Bölmek için tek görsel seç"}
        >
          Böl
        </button>
      </div>

      {result && (
        <ul className="tools__results">
          {result.map((path) => (
            <li key={path}>
              <code className="tools__path">{path}</code>
            </li>
          ))}
        </ul>
      )}

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}
    </section>
  );
}
