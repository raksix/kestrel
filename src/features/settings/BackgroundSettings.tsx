import { useEffect, useState } from "react";
import {
  backgroundStatus,
  quitApp,
  setBackground,
  type BackgroundStatus,
} from "../../lib/ipc";
import "./settings.css";

/**
 * Whether Kestrel keeps running, where it lives, and whether it starts with the
 * session.
 *
 * Closing to the tray is on by default because a capture tool has to be running
 * to answer a shortcut — without it, the first time the window is closed every
 * shortcut silently stops working, which reads as the shortcuts being broken.
 * The other two are off: one hides the app from where people look for it, the
 * other adds an entry to their login, and neither is a decision to make on
 * someone's behalf.
 */
export default function BackgroundSettings() {
  const [status, setStatus] = useState<BackgroundStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    backgroundStatus().then(setStatus).catch((e) => setError(String(e)));
  }, []);

  const change = (changes: Parameters<typeof setBackground>[0]) =>
    setBackground(changes)
      .then((next) => {
        setStatus(next);
        setError(null);
      })
      .catch((e) => setError(String(e)));

  if (!status) return null;

  return (
    <section className="card">
      <h2 className="card__title">Arka planda çalışma</h2>

      <label className="tasks__label">
        <input
          type="checkbox"
          checked={status.closeToTray}
          onChange={(e) => void change({ closeToTray: e.target.checked })}
        />
        <span>Pencereyi kapatınca arka planda kalsın</span>
      </label>
      <p className="card__hint">
        Kısayolların çalışması için Kestrel'in açık olması gerekir. Bu kapalıyken
        pencereyi kapatmak uygulamadan çıkar ve kısayollar sessizce çalışmaz olur.
      </p>

      {status.supportsMenuBarOnly && (
        <>
          <label className="tasks__label">
            <input
              type="checkbox"
              checked={status.menuBarOnly}
              onChange={(e) => void change({ menuBarOnly: e.target.checked })}
            />
            <span>Dock'ta görünmesin, sadece menü çubuğunda dursun</span>
          </label>
          <p className="card__hint">
            Açıkken uygulamaya yalnızca menü çubuğundaki simgeden ulaşılır —
            Dock'ta ve uygulama değiştiricide görünmez.
          </p>
        </>
      )}

      <label className="tasks__label">
        <input
          type="checkbox"
          checked={status.launchAtLogin}
          onChange={(e) => void change({ launchAtLogin: e.target.checked })}
        />
        <span>Oturum açılışında başlat</span>
      </label>
      <p className="card__hint">
        Bu kutu ayarlardan değil işletim sisteminden okunur; giriş öğesini
        sistem ayarlarından kaldırırsan burada da kapalı görünür.
      </p>

      {/* Closing the window no longer quits, so there has to be a way out that
          is not "find the tray icon" — especially for someone who has just
          hidden the dock icon. */}
      <button type="button" className="button" onClick={() => void quitApp()}>
        Kestrel'den çık
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
