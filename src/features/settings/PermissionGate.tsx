import { useEffect, useState } from "react";
import {
  openPermissionSettings,
  permissionStatus,
  requestScreenPermission,
  type PermissionStatus,
} from "../../lib/ipc";
import "./settings.css";

/**
 * Screen recording permission banner.
 *
 * macOS does not fail when the permission is missing — it quietly returns a
 * wallpaper image and an empty window list. That silent degradation is exactly
 * what makes "window capture does nothing" so confusing, so we surface it
 * loudly and poll for the fix.
 */
export default function PermissionGate({
  onGranted,
}: {
  onGranted?: () => void;
}) {
  const [status, setStatus] = useState<PermissionStatus | null>(null);
  const [asked, setAsked] = useState(false);

  useEffect(() => {
    permissionStatus().then(setStatus).catch(() => undefined);
  }, []);

  // macOS only applies a permission change after the app is relaunched, but
  // the status flips immediately — poll so the banner can tell the user that.
  useEffect(() => {
    if (status !== "denied") return;
    const timer = window.setInterval(() => {
      permissionStatus()
        .then((next) => {
          setStatus(next);
          if (next === "granted") onGranted?.();
        })
        .catch(() => undefined);
    }, 1500);
    return () => window.clearInterval(timer);
  }, [status, onGranted]);

  if (status === null || status === "granted" || status === "not_required") {
    return null;
  }

  return (
    <div className="permission permission--blocking" role="alert">
      <strong>Kestrel ekranı göremiyor</strong>
      <p className="card__hint" style={{ margin: 0 }}>
        Ekran görüntüsü alabilmek için macOS'un <b>Ekran Kaydı</b> iznine ihtiyacımız var.
        İzin verilmeden pencere listesi boş görünür ve yakalamalar yalnızca masaüstü arka
        planını içerir. Kestrel hiçbir şeyi kendi başına kaydetmez, hiçbir veriyi dışarı
        göndermez.
      </p>
      <div className="permission__actions">
        <button
          type="button"
          className="button button--primary"
          onClick={async () => {
            setAsked(true);
            setStatus(await requestScreenPermission());
          }}
        >
          İzin ver
        </button>
        <button type="button" className="button" onClick={() => void openPermissionSettings()}>
          Sistem Ayarları'nı aç
        </button>
      </div>
      {asked && (
        <p className="card__hint" style={{ margin: 0 }}>
          macOS bu izni yalnızca bir kez sorar. Pencere açılmadıysa Sistem Ayarları →
          Gizlilik ve Güvenlik → Ekran Kaydı bölümünden Kestrel'i işaretle, sonra
          uygulamayı yeniden başlat.
        </p>
      )}
    </div>
  );
}
